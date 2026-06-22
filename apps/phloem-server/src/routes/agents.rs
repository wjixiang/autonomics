//! Multi-agent management: list, create, delete agents and their conversation history.

use std::sync::Arc;

use axum::{
    Router,
    extract::{Path, State},
    routing::{delete, get, post},
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use agentik_types::messages::{ContentBlock, Role};

use crate::error::AppError;
use crate::services::agent_manager;
use crate::state::AppState;

// ── Response DTOs ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct AgentInfo {
    pub id: String,
    pub identity: String,
    pub status: String,
    pub last_active_ts: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct MessageView {
    pub id: String,
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCallView>,
}

#[derive(Debug, Serialize)]
pub struct ToolCallView {
    pub name: String,
    #[serde(skip_serializing_if = "serde_json::Value::is_null")]
    pub input: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<ToolResultView>,
}

#[derive(Debug, Serialize)]
pub struct ToolResultView {
    pub ok: bool,
    pub content: String,
}

// ── Request types ────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateAgentRequest {
    pub identity: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SendMessageRequest {
    pub content: String,
}

// ── Handlers ────────────────────────────────────────────────────────

/// GET /api/agents — list all agents with their status and last activity.
async fn list_agents(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<AgentInfo>>, AppError> {
    let agents = agent_manager::list_agents(&state)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;
    Ok(Json(agents))
}

/// POST /api/agents — create a new agent.
async fn create_agent(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateAgentRequest>,
) -> Result<Json<AgentInfo>, AppError> {
    let identity = req.identity.as_deref().unwrap_or("You are a helpful AI assistant.");
    let handle = agent_manager::get_or_create_agent(&state, None, Some(identity))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;
    let info = agent_info_from_handle(&handle, &state).await;
    Ok(Json(info))
}

/// DELETE /api/agents/:id — delete an agent and its snapshots.
async fn delete_agent(
    State(state): State<Arc<AppState>>,
    Path(agent_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    agent_manager::delete_agent(&state, agent_id)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}

/// GET /api/agents/:id/messages — load conversation history from the latest snapshot.
async fn get_messages(
    State(state): State<Arc<AppState>>,
    Path(agent_id): Path<Uuid>,
) -> Result<Json<Vec<MessageView>>, AppError> {
    let snapshot = state
        .storage
        .get_latest_snapshot(agent_id)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let messages = match snapshot {
        Some(snap) => flatten_memory(&snap.memory),
        None => Vec::new(),
    };

    Ok(Json(messages))
}

/// POST /api/agents/:id/send — send a message to a specific agent.
async fn send_to_agent(
    State(state): State<Arc<AppState>>,
    Path(agent_id): Path<Uuid>,
    Json(req): Json<SendMessageRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Ensure the agent exists in the pool
    let handle = agent_manager::get_or_create_agent(&state, Some(agent_id), None)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let _started = handle
        .send_message(&req.content)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(serde_json::json!({
        "agent_id": handle.id.to_string(),
        "status": handle.status().await,
    })))
}

// ── Helpers ─────────────────────────────────────────────────────────

fn flatten_memory(memory: &agentik_core::memory::Memory) -> Vec<MessageView> {
    let mut views = Vec::new();

    for item in &memory.items {
        // Emit summary as a system-context message
        if let Some(summary) = &item.summary {
            views.push(MessageView {
                id: Uuid::new_v4().to_string(),
                role: "system".to_string(),
                content: format!("<conversation-checkpoint>\n{}\n</conversation-checkpoint>", summary),
                thinking: None,
                tool_calls: Vec::new(),
            });
        }

        // Flatten each Message's ContentBlocks
        for msg in &item.messages {
            let mut text_parts = Vec::new();
            let mut thinking = None;
            let mut tool_calls = Vec::new();
            let mut pending_results: std::collections::HashMap<String, ToolResultView> =
                std::collections::HashMap::new();

            for block in &msg.content {
                match block {
                    ContentBlock::Text { text } => {
                        text_parts.push(text.clone());
                    }
                    ContentBlock::Thinking { thinking: t, .. } => {
                        thinking = Some(t.clone());
                    }
                    ContentBlock::ToolUse {
                        id, name, input, ..
                    } => {
                        // Attach any pending result for this tool_use_id
                        let result = pending_results.remove(id);
                        tool_calls.push(ToolCallView {
                            name: name.clone(),
                            input: input.clone(),
                            result,
                        });
                    }
                    ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                    } => {
                        let tr = ToolResultView {
                            ok: !is_error.unwrap_or(false),
                            content: content.clone().unwrap_or_default(),
                        };
                        // Try to attach to the last matching tool call
                        if let Some(tc) = tool_calls.iter_mut().last() {
                            if tc.name == "" || tc.result.is_none() {
                                tc.result = Some(tr);
                            }
                        } else {
                            // Store for next ToolUse block
                            pending_results.insert(tool_use_id.clone(), tr);
                        }
                    }
                    ContentBlock::Image { .. } => {
                        // Skip images in history view
                    }
                }
            }

            // Only emit a MessageView if there's content
            // (skip bare tool-result messages that have no text/thinking/tool_calls)
            if !text_parts.is_empty()
                || thinking.is_some()
                || !tool_calls.is_empty()
            {
                let role = match &msg.role {
                    Role::User => "user",
                    Role::Assistant => "assistant",
                };
                views.push(MessageView {
                    id: msg.id.clone(),
                    role: role.to_string(),
                    content: text_parts.join("\n"),
                    thinking,
                    tool_calls,
                });
            }
        }
    }

    views
}

async fn agent_info_from_handle(
    handle: &agent_manager::AgentHandle,
    state: &AppState,
) -> AgentInfo {
    let last_ts = state
        .storage
        .get_latest_snapshot(handle.id)
        .await
        .ok()
        .flatten()
        .map(|s| s.ts);

    AgentInfo {
        id: handle.id.to_string(),
        identity: handle.identity.clone(),
        status: handle.status().await,
        last_active_ts: last_ts,
    }
}

/// GET /api/agents/:id/stream — SSE live stream for a specific agent.
async fn stream_agent(
    State(state): State<Arc<AppState>>,
    Path(agent_id): Path<Uuid>,
) -> Result<
    axum::response::sse::Sse<
        impl futures::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>,
    >,
    AppError,
>
where
    std::convert::Infallible: Send,
{
    let handle = agent_manager::get_or_create_agent(&state, Some(agent_id), None)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    // Subscribe-only: no replay.
    let rx = state.event_broker.subscribe();
    Ok(crate::sse::agent_event_stream(handle.id, Vec::new(), rx))
}

// ── Router ─────────────────────────────────────────────────────────

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/agents", get(list_agents).post(create_agent))
        .route(
            "/api/agents/{id}/messages",
            get(get_messages),
        )
        .route(
            "/api/agents/{id}/send",
            post(send_to_agent),
        )
        .route(
            "/api/agents/{id}/stream",
            get(stream_agent),
        )
        .route(
            "/api/agents/{id}",
            delete(delete_agent),
        )
}
