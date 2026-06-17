//! HTTP + SSE client for the agentik control plane.
//!
//! Talks to the `agentik-runtime` daemon's REST API (see
//! `agentik-runtime/src/http.rs`). All payloads are JSON (the `agentik-api`
//! wire types); the two event streams (`stream_events`, `watch_skills`) are
//! consumed as Server-Sent Events.

use std::time::Duration;

use eventsource_stream::Eventsource;
use serde::{de::DeserializeOwned, Serialize};
use tokio_stream::StreamExt as _;

use agentik_api::{
    AgentLifecycleStatus, AgentSpawnOpts, ContentBlock, ModelConfig, ProcessEvent,
    SkillChangeNotificationWire, SkillTreeNodeWire, SkillWire,
};

const BASE_PATH: &str = "/api/v1";

#[derive(Debug, thiserror::Error)]
pub enum ControlClientError {
    #[error("invalid endpoint: {0}")]
    InvalidAddress(String),
    #[error("HTTP connection error: {0}")]
    Connection(#[from] reqwest::Error),
    #[error("daemon returned status {0}: {1}")]
    Status(u16, String),
    #[error("daemon discovery file not found — is the daemon running?")]
    NoDaemon,
    #[error("payload (de)serialisation error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("server returned an error: {0}")]
    Server(String),
    #[error("unexpected empty response from server")]
    Empty,
}

/// Info about one managed agent, returned by [`ControlClient::list_agents`].
#[derive(Debug, Clone)]
pub struct AgentInfo {
    pub agent_id: uuid::Uuid,
    pub kind: String,
    pub status: AgentLifecycleStatus,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct AgentInfoDto {
    agent_id: String,
    kind: String,
    status: AgentLifecycleStatus,
}

#[derive(serde::Serialize)]
struct SpawnAgentReq<'a> {
    kind: &'a str,
    opts: &'a AgentSpawnOpts,
}

#[derive(serde::Deserialize)]
struct AgentIdDto {
    agent_id: String,
}

#[derive(serde::Serialize)]
struct InjectMessageReq<'a> {
    content: &'a [ContentBlock],
}

#[derive(serde::Deserialize)]
struct StatusDto {
    status: AgentLifecycleStatus,
}

#[derive(serde::Serialize)]
struct DirReq<'a> {
    dir: &'a str,
}

#[derive(serde::Deserialize)]
struct ImportResultDto {
    count: u32,
    #[serde(default)]
    error: Option<String>,
}

#[derive(serde::Deserialize)]
#[allow(dead_code)]
struct ReloadResultDto {
    skill: Option<SkillWire>,
    #[serde(default)]
    not_changed: bool,
    #[serde(default)]
    error: Option<String>,
}

#[derive(serde::Serialize)]
struct SkillListReq {
    user_invocable_only: bool,
    model_invocable_only: bool,
}

/// HTTP/SSE client for the agentik control plane.
pub struct ControlClient {
    http: reqwest::Client,
    base: String,
}

impl ControlClient {
    /// Connect to a daemon at the given base endpoint (e.g. `"http://127.0.0.1:54321"`).
    pub async fn connect(addr: &str) -> Result<Self, ControlClientError> {
        let base = addr.trim_end_matches('/').to_string();
        // Validate by constructing a URL.
        let _ = reqwest::Url::parse(&format!("{base}/"))
            .map_err(|e| ControlClientError::InvalidAddress(e.to_string()))?;
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| ControlClientError::InvalidAddress(e.to_string()))?;
        Ok(Self { http, base })
    }

    fn url(&self, path: &str) -> String {
        format!("{base}{BASE_PATH}{path}", base = self.base)
    }

    async fn decode<T: DeserializeOwned>(
        &self,
        resp: reqwest::Response,
    ) -> Result<T, ControlClientError> {
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(ControlClientError::Status(status, body));
        }
        Ok(resp.json().await?)
    }

    // ── Agent lifecycle ──

    /// Spawn an agent by registered kind. Returns the new agent's id.
    pub async fn spawn_agent(
        &mut self,
        kind: &str,
        opts: &AgentSpawnOpts,
    ) -> Result<uuid::Uuid, ControlClientError> {
        let dto: AgentIdDto = self
            .decode(
                self.http
                    .post(self.url("/agents"))
                    .json(&SpawnAgentReq { kind, opts })
                    .send()
                    .await?,
            )
            .await?;
        uuid::Uuid::parse_str(&dto.agent_id)
            .map_err(|e| ControlClientError::Server(format!("invalid agent_id from server: {e}")))
    }

    pub async fn start_agent(&mut self, agent_id: uuid::Uuid) -> Result<(), ControlClientError> {
        self.post_empty_status(&format!("/agents/{}/start", agent_id)).await
    }

    pub async fn stop_agent(&mut self, agent_id: uuid::Uuid) -> Result<(), ControlClientError> {
        self.post_empty_status(&format!("/agents/{}/stop", agent_id)).await
    }

    pub async fn restart_agent(&mut self, agent_id: uuid::Uuid) -> Result<(), ControlClientError> {
        self.post_empty_status(&format!("/agents/{}/restart", agent_id)).await
    }

    pub async fn inject_message(
        &mut self,
        agent_id: uuid::Uuid,
        content: Vec<ContentBlock>,
    ) -> Result<(), ControlClientError> {
        self.post_empty(
            &format!("/agents/{}/messages", agent_id),
            &InjectMessageReq { content: &content },
        )
        .await
    }

    pub async fn list_agents(&mut self) -> Result<Vec<AgentInfo>, ControlClientError> {
        let dtos: Vec<AgentInfoDto> = self
            .decode(self.http.get(self.url("/agents")).send().await?)
            .await?;
        dtos.into_iter()
            .map(|d| {
                let agent_id = uuid::Uuid::parse_str(&d.agent_id)
                    .map_err(|e| ControlClientError::Server(format!("invalid agent_id: {e}")))?;
                Ok(AgentInfo {
                    agent_id,
                    kind: d.kind,
                    status: d.status,
                })
            })
            .collect()
    }

    pub async fn get_status(
        &mut self,
        agent_id: uuid::Uuid,
    ) -> Result<AgentLifecycleStatus, ControlClientError> {
        let dto: StatusDto = self
            .decode(
                self.http
                    .get(self.url(&format!("/agents/{}/status", agent_id)))
                    .send()
                    .await?,
            )
            .await?;
        Ok(dto.status)
    }

    // ── Pool / kinds ──

    pub async fn get_pool_models(&mut self) -> Result<Vec<String>, ControlClientError> {
        self.decode(self.http.get(self.url("/pool/models")).send().await?)
            .await
    }

    pub async fn configure_pool(
        &mut self,
        config: &ModelConfig,
    ) -> Result<(), ControlClientError> {
        self.post_empty("/pool", config).await
    }

    pub async fn list_kinds(&mut self) -> Result<Vec<String>, ControlClientError> {
        self.decode(self.http.get(self.url("/kinds")).send().await?)
            .await
    }

    // ── Lifecycle ──

    /// Gracefully shut the daemon down.
    pub async fn shutdown(&mut self) -> Result<(), ControlClientError> {
        self.post_empty_status("/shutdown").await
    }

    /// Liveness probe (used by `daemon status`).
    pub async fn ping(&mut self) -> Result<(), ControlClientError> {
        self.decode::<Ackless>(self.http.get(self.url("/health")).send().await?)
            .await?;
        Ok(())
    }

    // ── Events (SSE) ──

    /// Subscribe to the aggregated event stream. Each item is a parsed
    /// [`ProcessEvent`]; the stream ends when the daemon closes it.
    pub async fn stream_events(
        &mut self,
    ) -> Result<
        impl tokio_stream::Stream<Item = Result<ProcessEvent, ControlClientError>>,
        ControlClientError,
    > {
        self.sse_stream::<ProcessEvent>("/events").await
    }

    // ── Skill tree management ──

    pub async fn list_skills(
        &mut self,
        user_invocable_only: bool,
        model_invocable_only: bool,
    ) -> Result<Vec<SkillWire>, ControlClientError> {
        let resp = self
            .http
            .get(self.url("/skills"))
            .query(&SkillListReq {
                user_invocable_only,
                model_invocable_only,
            })
            .send()
            .await?;
        self.decode(resp).await
    }

    pub async fn get_skill(&mut self, name: &str) -> Result<Option<SkillWire>, ControlClientError> {
        let resp = self
            .http
            .get(self.url(&format!("/skills/{name}")))
            .send()
            .await?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let skill: SkillWire = self.decode(resp).await?;
        Ok(Some(skill))
    }

    pub async fn get_skill_tree(
        &mut self,
    ) -> Result<Option<SkillTreeNodeWire>, ControlClientError> {
        let resp = self.http.get(self.url("/skills/tree")).send().await?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let tree: SkillTreeNodeWire = self.decode(resp).await?;
        Ok(Some(tree))
    }

    pub async fn reload_skill(
        &mut self,
        name: &str,
    ) -> Result<Option<SkillWire>, ControlClientError> {
        let resp = self
            .http
            .post(self.url(&format!("/skills/{name}/reload")))
            .send()
            .await?;
        let dto: ReloadResultDto = self.decode(resp).await?;
        if let Some(e) = dto.error {
            return Err(ControlClientError::Server(e));
        }
        Ok(dto.skill)
    }

    pub async fn import_skills(&mut self, dir: &str) -> Result<u32, ControlClientError> {
        let resp = self
            .http
            .post(self.url("/skills/import"))
            .json(&DirReq { dir })
            .send()
            .await?;
        let dto: ImportResultDto = self.decode(resp).await?;
        result_or_error(dto)
    }

    pub async fn export_skills(&mut self, dir: &str) -> Result<u32, ControlClientError> {
        let resp = self
            .http
            .post(self.url("/skills/export"))
            .json(&DirReq { dir })
            .send()
            .await?;
        let dto: ImportResultDto = self.decode(resp).await?;
        result_or_error(dto)
    }

    pub async fn watch_skills(
        &mut self,
    ) -> Result<
        impl tokio_stream::Stream<Item = Result<SkillChangeNotificationWire, ControlClientError>>,
        ControlClientError,
    > {
        self.sse_stream::<SkillChangeNotificationWire>("/skills/events")
            .await
    }

    // ── Internal helpers ──

    async fn post_empty<B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<(), ControlClientError> {
        let resp = self.http.post(self.url(path)).json(body).send().await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            return Err(ControlClientError::Status(status, text));
        }
        Ok(())
    }

    /// POST with no request body, expecting an Ack-style JSON that we ignore.
    async fn post_empty_status(&self, path: &str) -> Result<(), ControlClientError> {
        let resp = self.http.post(self.url(path)).send().await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            return Err(ControlClientError::Status(status, text));
        }
        Ok(())
    }

    async fn sse_stream<T: DeserializeOwned + Send + 'static>(
        &self,
        path: &str,
    ) -> Result<impl tokio_stream::Stream<Item = Result<T, ControlClientError>>, ControlClientError>
    {
        let resp = self.http.get(self.url(path)).send().await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            return Err(ControlClientError::Status(status, text));
        }
        let byte_stream = resp.bytes_stream().eventsource();
        let stream = byte_stream.filter_map(|ev| match ev {
            Ok(event) => match serde_json::from_str::<T>(&event.data) {
                Ok(parsed) => Some(Ok(parsed)),
                Err(e) => Some(Err(ControlClientError::Serde(e))),
            },
            Err(e) => Some(Err(ControlClientError::Server(e.to_string()))),
        });
        Ok(stream)
    }
}

#[derive(serde::Deserialize)]
struct Ackless {}

fn result_or_error(dto: ImportResultDto) -> Result<u32, ControlClientError> {
    match dto.error {
        None => Ok(dto.count),
        Some(e) => Err(ControlClientError::Server(e)),
    }
}
