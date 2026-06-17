//! REST + SSE control-plane HTTP server.
//!
//! [`router`] builds the axum app exposing the agent lifecycle / observation
//! / skill-tree API as JSON-over-HTTP, with two SSE endpoints for the event
//! streams (`/events`, `/skills/events`). Replaces the former gRPC control
//! plane; the internal skill registry gRPC (used by agents for `activate_skill`)
//! is unaffected.
//!
//! Bodies use the serde wire types in `agentik-api` directly — no `*_json`
//! string wrapping.

use std::path::Path;
use std::sync::Arc;

use axum::{
    extract::{Path as AxumPath, Query, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Json,
    },
    routing::{get, post, put},
    Router,
};
use serde::{Deserialize, Serialize};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt as _;
use tokio_util::sync::CancellationToken;
use tower_http::cors::CorsLayer;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use agentik_api::{
    AgentLifecycleStatus, AgentSpawnOpts, ContentBlock, ModelConfig, PoolEntry, ProcessEvent,
    ProcessExitStatus, ProviderConfig, SkillChangeNotificationWire, SkillChangeWire,
    SkillReferenceWire, SkillTreeNodeWire, SkillWire,
};
use agentik_skill::{Skill, SkillTreeNode};
use agentik_skill_client::SkillRegistryClient;
use agentik_skill_server::store::SkillStore as _;
use agentik_skill_server::{SkillChangeType, SqliteSkillStore};

// `ImageSource` nests under `ContentBlock`; pull it in for the OpenAPI
// `components(schemas(...))` list.
#[allow(unused_imports)]
use agentik_types::ImageSource;

use tokio::sync::Mutex;

use crate::kinds;
use crate::process::AgentManager;

/// Shared state for all handlers.
#[derive(Clone)]
pub struct HttpState {
    inner: Arc<HttpStateInner>,
}

struct HttpStateInner {
    pm: AgentManager,
    store: Option<Arc<SqliteSkillStore>>,
    skill_client: Option<Arc<Mutex<SkillRegistryClient>>>,
    shutdown: CancellationToken,
}

impl HttpState {
    pub fn new(
        pm: AgentManager,
        store: Option<Arc<SqliteSkillStore>>,
        skill_client: Option<Arc<Mutex<SkillRegistryClient>>>,
        shutdown: CancellationToken,
    ) -> Self {
        Self {
            inner: Arc::new(HttpStateInner {
                pm,
                store,
                skill_client,
                shutdown,
            }),
        }
    }

    fn store(&self) -> Result<&Arc<SqliteSkillStore>, (StatusCode, String)> {
        self.inner
            .store
            .as_ref()
            .ok_or_else(|| (StatusCode::SERVICE_UNAVAILABLE, "skill store not configured".into()))
    }

    /// Refresh the built-in `coder` kind from the current store contents, so
    /// that agents spawned after a skill import see the new tree.
    async fn refresh_coder_kind(&self, store: &Arc<SqliteSkillStore>) {
        match kinds::coder_kind(store, self.inner.skill_client.clone()).await {
            Ok(blueprint) => {
                self.inner.pm.registry().register(blueprint);
                tracing::info!("refreshed 'coder' kind after skill change");
            }
            Err(e) => {
                tracing::warn!(error = %e, "failed to refresh 'coder' kind after skill change");
            }
        }
    }
}

/// OpenAPI document for the control-plane REST API, served at
/// `/api-docs/openapi.json` and rendered as Swagger UI at `/docs`.
#[derive(OpenApi)]
#[openapi(
    info(
        title = "agentik control plane",
        version = "0.1.0",
        description = "REST + SSE API for the agentik runtime daemon \
                       (agent lifecycle, model pool, skill-tree management).",
    ),
    paths(
        health, shutdown,
        spawn_agent, list_agents, start_agent, stop_agent, restart_agent,
        inject_message, get_status, stream_events,
        pool_models, configure_pool, list_kinds,
        list_skills, skill_tree, get_skill, reload_skill, import_skills,
        export_skills, watch_skills,
    ),
    components(schemas(
        SpawnAgentBody, AgentIdBody, InjectMessageBody, DirBody, ImportResultBody,
        AckBody, StatusBody, AgentInfoBody, SkillListQuery, ReloadResultBody,
        AgentLifecycleStatus, AgentSpawnOpts, ContentBlock, ImageSource,
        ModelConfig, ProviderConfig, PoolEntry,
        SkillWire, SkillTreeNodeWire, SkillReferenceWire,
        SkillChangeWire, SkillChangeNotificationWire,
        ProcessEvent, ProcessExitStatus,
    )),
    tags(
        (name = "system", description = "health / shutdown"),
        (name = "agents", description = "agent lifecycle & observation"),
        (name = "pool",   description = "model pool / kinds"),
        (name = "skills", description = "skill-tree management"),
    ),
)]
pub struct ApiDoc;

/// Build the control-plane axum router.
pub fn router(state: HttpState) -> Router {
    let cors = CorsLayer::very_permissive();

    let swagger = SwaggerUi::new("/docs")
        .url("/api-docs/openapi.json", ApiDoc::openapi());

    Router::new()
        .merge(swagger)
        // ── health / lifecycle ──
        .route("/api/v1/health", get(health))
        .route("/api/v1/shutdown", post(shutdown))
        // ── agents ──
        .route("/api/v1/agents", post(spawn_agent).get(list_agents))
        .route("/api/v1/agents/{id}/start", post(start_agent))
        .route("/api/v1/agents/{id}/stop", post(stop_agent))
        .route("/api/v1/agents/{id}/restart", post(restart_agent))
        .route("/api/v1/agents/{id}/messages", post(inject_message))
        .route("/api/v1/agents/{id}/status", get(get_status))
        // ── pool / kinds ──
        .route("/api/v1/pool/models", get(pool_models))
        .route("/api/v1/pool", put(configure_pool))
        .route("/api/v1/kinds", get(list_kinds))
        // ── events (SSE) ──
        .route("/api/v1/events", get(stream_events))
        // ── skills ──
        .route("/api/v1/skills", get(list_skills))
        .route("/api/v1/skills/tree", get(skill_tree))
        .route("/api/v1/skills/events", get(watch_skills))
        .route("/api/v1/skills/{name}", get(get_skill))
        .route("/api/v1/skills/{name}/reload", post(reload_skill))
        .route("/api/v1/skills/import", post(import_skills))
        .route("/api/v1/skills/export", post(export_skills))
        .layer(cors)
        .with_state(state)
}

// ── Request / response bodies ───────────────────────────────────

#[derive(Debug, Deserialize, utoipa::ToSchema)]
struct SpawnAgentBody {
    kind: String,
    #[serde(default)]
    opts: AgentSpawnOpts,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
struct AgentIdBody {
    agent_id: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
struct InjectMessageBody {
    content: Vec<ContentBlock>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
struct DirBody {
    dir: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
struct ImportResultBody {
    count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
struct AckBody {
    ok: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    error: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
struct StatusBody {
    status: AgentLifecycleStatus,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
struct SkillListQuery {
    #[serde(default)]
    user_invocable_only: bool,
    #[serde(default)]
    model_invocable_only: bool,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
struct ReloadResultBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    skill: Option<SkillWire>,
    not_changed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

fn ack(ok: bool, error: impl Into<String>) -> AckBody {
    AckBody {
        ok,
        error: error.into(),
    }
}

fn ok_ack() -> AckBody {
    ack(true, "")
}

fn err_status<E: std::fmt::Display>(code: StatusCode, e: E) -> (StatusCode, String) {
    (code, e.to_string())
}

// ── Handlers ────────────────────────────────────────────────────

#[utoipa::path(
    get,
    path = "/api/v1/health",
    responses((status = 200, body = AckBody, description = "daemon is alive")),
    tag = "system",
)]
async fn health() -> impl IntoResponse {
    Json(ack(true, ""))
}

#[utoipa::path(
    post,
    path = "/api/v1/shutdown",
    responses((status = 200, body = AckBody, description = "shutdown signalled")),
    tag = "system",
)]
async fn shutdown(State(state): State<HttpState>) -> impl IntoResponse {
    tracing::info!("shutdown requested via control plane");
    state.inner.shutdown.cancel();
    Json(ok_ack())
}

#[utoipa::path(
    post,
    path = "/api/v1/agents",
    request_body = SpawnAgentBody,
    responses(
        (status = 200, body = AgentIdBody, description = "spawned agent id"),
        (status = 400, body = String, description = "spawn failed"),
    ),
    tag = "agents",
)]
async fn spawn_agent(
    State(state): State<HttpState>,
    Json(body): Json<SpawnAgentBody>,
) -> Result<Json<AgentIdBody>, (StatusCode, String)> {
    match state.inner.pm.spawn_by_kind(&body.kind, body.opts).await {
        Ok(id) => Ok(Json(AgentIdBody {
            agent_id: id.to_string(),
        })),
        Err(e) => Err(err_status(StatusCode::BAD_REQUEST, e)),
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/agents/{id}/start",
    params(("id" = String, Path, description = "agent UUID")),
    responses(
        (status = 200, body = AckBody),
        (status = 400, body = String, description = "invalid agent id"),
    ),
    tag = "agents",
)]
async fn start_agent(
    State(state): State<HttpState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<AckBody>, (StatusCode, String)> {
    let id = parse_id(&id)?;
    Ok(Json(to_ack(state.inner.pm.start(&id))))
}

#[utoipa::path(
    post,
    path = "/api/v1/agents/{id}/stop",
    params(("id" = String, Path, description = "agent UUID")),
    responses(
        (status = 200, body = AckBody),
        (status = 400, body = String, description = "invalid agent id"),
    ),
    tag = "agents",
)]
async fn stop_agent(
    State(state): State<HttpState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<AckBody>, (StatusCode, String)> {
    let id = parse_id(&id)?;
    Ok(Json(to_ack(state.inner.pm.stop(&id))))
}

#[utoipa::path(
    post,
    path = "/api/v1/agents/{id}/restart",
    params(("id" = String, Path, description = "agent UUID")),
    responses(
        (status = 200, body = AckBody),
        (status = 400, body = String, description = "invalid agent id"),
    ),
    tag = "agents",
)]
async fn restart_agent(
    State(state): State<HttpState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<AckBody>, (StatusCode, String)> {
    let id = parse_id(&id)?;
    Ok(Json(to_ack(state.inner.pm.restart(&id))))
}

#[utoipa::path(
    post,
    path = "/api/v1/agents/{id}/messages",
    params(("id" = String, Path, description = "agent UUID")),
    request_body = InjectMessageBody,
    responses(
        (status = 200, body = AckBody),
        (status = 400, body = String, description = "invalid agent id"),
    ),
    tag = "agents",
)]
async fn inject_message(
    State(state): State<HttpState>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<InjectMessageBody>,
) -> Result<Json<AckBody>, (StatusCode, String)> {
    let id = parse_id(&id)?;
    Ok(Json(to_ack(state.inner.pm.inject_message(&id, body.content))))
}

#[utoipa::path(
    get,
    path = "/api/v1/agents",
    responses((status = 200, body = Vec<AgentInfoBody>, description = "managed agents")),
    tag = "agents",
)]
async fn list_agents(State(state): State<HttpState>) -> Json<Vec<AgentInfoBody>> {
    let snapshot = state.inner.pm.snapshot().await;
    Json(
        snapshot
            .into_iter()
            .map(|(id, kind, status)| AgentInfoBody {
                agent_id: id.to_string(),
                kind,
                status,
            })
            .collect(),
    )
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
struct AgentInfoBody {
    agent_id: String,
    kind: String,
    status: AgentLifecycleStatus,
}

#[utoipa::path(
    get,
    path = "/api/v1/agents/{id}/status",
    params(("id" = String, Path, description = "agent UUID")),
    responses(
        (status = 200, body = StatusBody),
        (status = 400, body = String, description = "invalid agent id"),
        (status = 404, body = String, description = "agent not found"),
    ),
    tag = "agents",
)]
async fn get_status(
    State(state): State<HttpState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<StatusBody>, (StatusCode, String)> {
    let id = parse_id(&id)?;
    match state.inner.pm.status(&id) {
        Ok(status) => Ok(Json(StatusBody { status })),
        Err(e) => Err(err_status(StatusCode::NOT_FOUND, e)),
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/pool/models",
    responses((status = 200, body = Vec<String>, description = "model names in the pool")),
    tag = "pool",
)]
async fn pool_models(State(state): State<HttpState>) -> Json<Vec<String>> {
    Json(state.inner.pm.pool_model_names().await)
}

#[utoipa::path(
    put,
    path = "/api/v1/pool",
    request_body = ModelConfig,
    responses((status = 200, body = AckBody, description = "pool reconfigured")),
    tag = "pool",
)]
async fn configure_pool(
    State(state): State<HttpState>,
    Json(cfg): Json<ModelConfig>,
) -> Result<Json<AckBody>, (StatusCode, String)> {
    let res = state.inner.pm.configure_pool(&cfg).await;
    Ok(Json(to_ack(res)))
}

#[utoipa::path(
    get,
    path = "/api/v1/kinds",
    responses((status = 200, body = Vec<String>, description = "registered agent kind names")),
    tag = "pool",
)]
async fn list_kinds(State(state): State<HttpState>) -> Json<Vec<String>> {
    Json(state.inner.pm.registry().list())
}

#[utoipa::path(
    get,
    path = "/api/v1/events",
    responses((status = 200, content_type = "text/event-stream",
        description = "SSE stream of ProcessEvent frames; each `data:` line is a JSON ProcessEvent")),
    tag = "agents",
)]
async fn stream_events(
    State(state): State<HttpState>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let rx = state.inner.pm.events();
    let stream = BroadcastStream::new(rx).filter_map(|result| {
        result.ok().map(|event: ProcessEvent| {
            let data = serde_json::to_string(&event).unwrap_or_default();
            Ok::<_, std::convert::Infallible>(Event::default().data(data))
        })
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

// ── Skill handlers ──────────────────────────────────────────────

#[utoipa::path(
    get,
    path = "/api/v1/skills",
    params(
        ("user_invocable_only" = Option<bool>, Query, description = "filter to user-invocable skills"),
        ("model_invocable_only" = Option<bool>, Query, description = "filter to model-invocable skills"),
    ),
    responses(
        (status = 200, body = Vec<SkillWire>),
        (status = 503, body = String, description = "skill store not configured"),
    ),
    tag = "skills",
)]
async fn list_skills(
    State(state): State<HttpState>,
    Query(q): Query<SkillListQuery>,
) -> Result<Json<Vec<SkillWire>>, (StatusCode, String)> {
    let store = state.store()?.clone();
    let tree = store
        .skill_tree()
        .await
        .map_err(|e| err_status(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let mut wires = Vec::new();
    flatten_tree(&tree, &mut wires);
    let filtered: Vec<SkillWire> = wires
        .into_iter()
        .filter(|w| !q.user_invocable_only || w.user_invocable)
        .filter(|w| !q.model_invocable_only || w.model_invocable)
        .collect();
    Ok(Json(filtered))
}

#[utoipa::path(
    get,
    path = "/api/v1/skills/{name}",
    params(("name" = String, Path, description = "skill name or alias")),
    responses(
        (status = 200, body = SkillWire),
        (status = 404, body = String, description = "skill not found"),
        (status = 503, body = String, description = "skill store not configured"),
    ),
    tag = "skills",
)]
async fn get_skill(
    State(state): State<HttpState>,
    AxumPath(name): AxumPath<String>,
) -> Result<Json<SkillWire>, (StatusCode, String)> {
    let store = state.store()?;
    let skill = store
        .get(&name)
        .await
        .map_err(|e| err_status(StatusCode::NOT_FOUND, e))?;
    let dotpath = store
        .skill_tree()
        .await
        .ok()
        .and_then(|t| find_dotpath(&t, &skill.metadata.name))
        .unwrap_or_default();
    Ok(Json(skill_to_wire(&dotpath, &skill)))
}

#[utoipa::path(
    get,
    path = "/api/v1/skills/tree",
    responses(
        (status = 200, body = SkillTreeNodeWire),
        (status = 404, body = String, description = "skill tree is empty"),
        (status = 503, body = String, description = "skill store not configured"),
    ),
    tag = "skills",
)]
async fn skill_tree(State(state): State<HttpState>) -> Result<Json<SkillTreeNodeWire>, (StatusCode, String)> {
    let store = state.store()?;
    let tree = store
        .skill_tree()
        .await
        .map_err(|e| err_status(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let root = tree
        .root
        .as_ref()
        .ok_or_else(|| (StatusCode::NOT_FOUND, "skill tree is empty".to_string()))?;
    Ok(Json(tree_to_wire(root)))
}

#[utoipa::path(
    post,
    path = "/api/v1/skills/{name}/reload",
    params(("name" = String, Path, description = "skill name")),
    responses(
        (status = 200, body = ReloadResultBody),
        (status = 503, body = String, description = "skill store not configured"),
    ),
    tag = "skills",
)]
async fn reload_skill(
    State(state): State<HttpState>,
    AxumPath(name): AxumPath<String>,
) -> Result<Json<ReloadResultBody>, (StatusCode, String)> {
    let store = state.store()?;
    match store.reload(&name).await {
        Ok(Some(skill)) => {
            let dotpath = store
                .skill_tree()
                .await
                .ok()
                .and_then(|t| find_dotpath(&t, &skill.metadata.name))
                .unwrap_or_default();
            Ok(Json(ReloadResultBody {
                skill: Some(skill_to_wire(&dotpath, &skill)),
                not_changed: false,
                error: None,
            }))
        }
        Ok(None) => Ok(Json(ReloadResultBody {
            skill: None,
            not_changed: true,
            error: None,
        })),
        Err(e) => Ok(Json(ReloadResultBody {
            skill: None,
            not_changed: false,
            error: Some(e.to_string()),
        })),
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/skills/import",
    request_body = DirBody,
    responses(
        (status = 200, body = ImportResultBody, description = "imported count (refreshes the coder kind)"),
        (status = 503, body = String, description = "skill store not configured"),
    ),
    tag = "skills",
)]
async fn import_skills(
    State(state): State<HttpState>,
    Json(body): Json<DirBody>,
) -> Result<Json<ImportResultBody>, (StatusCode, String)> {
    let store = state.store()?.clone();
    match store.import_from_dir(Path::new(&body.dir)).await {
        Ok(count) => {
            state.refresh_coder_kind(&store).await;
            Ok(Json(ImportResultBody {
                count: count as u32,
                error: None,
            }))
        }
        Err(e) => Ok(Json(ImportResultBody {
            count: 0,
            error: Some(e.to_string()),
        })),
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/skills/export",
    request_body = DirBody,
    responses(
        (status = 200, body = ImportResultBody, description = "exported count"),
        (status = 503, body = String, description = "skill store not configured"),
    ),
    tag = "skills",
)]
async fn export_skills(
    State(state): State<HttpState>,
    Json(body): Json<DirBody>,
) -> Result<Json<ImportResultBody>, (StatusCode, String)> {
    let store = state.store()?;
    match store.export_to_dir(Path::new(&body.dir)).await {
        Ok(count) => Ok(Json(ImportResultBody {
            count: count as u32,
            error: None,
        })),
        Err(e) => Ok(Json(ImportResultBody {
            count: 0,
            error: Some(e.to_string()),
        })),
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/skills/events",
    responses((status = 200, content_type = "text/event-stream",
        description = "SSE stream of skill-change notifications; each `data:` line is a JSON SkillChangeNotificationWire")),
    tag = "skills",
)]
async fn watch_skills(
    State(state): State<HttpState>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, std::convert::Infallible>>>, (StatusCode, String)>
{
    let store = state.store()?;
    let rx = store.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|result| {
        result.ok().map(|notif| {
            let change = match notif.change_type {
                SkillChangeType::Added => SkillChangeWire::Added,
                SkillChangeType::Modified => SkillChangeWire::Modified,
                SkillChangeType::Removed => SkillChangeWire::Removed,
            };
            let wire = SkillChangeNotificationWire {
                change_type: change,
                skill_name: notif.skill_name,
            };
            let data = serde_json::to_string(&wire).unwrap_or_default();
            Ok::<_, std::convert::Infallible>(Event::default().data(data))
        })
    });
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

// ── Helpers ─────────────────────────────────────────────────────

fn parse_id(s: &str) -> Result<uuid::Uuid, (StatusCode, String)> {
    uuid::Uuid::parse_str(s).map_err(|e| err_status(StatusCode::BAD_REQUEST, format!("invalid agent_id '{s}': {e}")))
}

fn to_ack(res: Result<(), crate::process::ProcessError>) -> AckBody {
    match res {
        Ok(()) => ok_ack(),
        Err(e) => ack(false, e.to_string()),
    }
}

// ── Skill ↔ wire mapping ────────────────────────────────────────

fn skill_to_wire(dotpath: &str, skill: &Skill) -> SkillWire {
    SkillWire {
        dotpath: dotpath.to_string(),
        name: skill.metadata.name.clone(),
        description: skill.metadata.description.clone(),
        aliases: skill.metadata.aliases.clone(),
        when_to_use: skill.metadata.when_to_use.clone(),
        argument_hint: skill.metadata.argument_hint.clone(),
        user_invocable: skill.metadata.user_invocable,
        model_invocable: skill.metadata.model_invocable,
        allowed_tools: skill.policy.allowed_tools.iter().cloned().collect(),
        body: skill.body.clone(),
        references: skill
            .references
            .iter()
            .map(|r| SkillReferenceWire {
                name: r.name.clone(),
                content: r.content.clone(),
            })
            .collect(),
        activation_paths: skill.activation_paths.clone(),
    }
}

fn tree_to_wire(node: &SkillTreeNode) -> SkillTreeNodeWire {
    SkillTreeNodeWire {
        skill: skill_to_wire(&node.dotpath, &node.skill),
        dotpath: node.dotpath.clone(),
        children: node.children.iter().map(tree_to_wire).collect(),
    }
}

fn flatten_tree(tree: &agentik_skill::SkillTree, out: &mut Vec<SkillWire>) {
    if let Some(root) = &tree.root {
        flatten_node(root, out);
    }
}

fn flatten_node(node: &SkillTreeNode, out: &mut Vec<SkillWire>) {
    out.push(skill_to_wire(&node.dotpath, &node.skill));
    for child in &node.children {
        flatten_node(child, out);
    }
}

fn find_dotpath(tree: &agentik_skill::SkillTree, name: &str) -> Option<String> {
    tree.root.as_ref().and_then(|n| find_dotpath_node(n, name))
}

fn find_dotpath_node(node: &SkillTreeNode, name: &str) -> Option<String> {
    if node.skill.metadata.name == name {
        return Some(node.dotpath.clone());
    }
    for child in &node.children {
        if let Some(dp) = find_dotpath_node(child, name) {
            return Some(dp);
        }
    }
    None
}
