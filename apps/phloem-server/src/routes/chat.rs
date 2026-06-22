//! Simplified chat endpoints: single-agent send + stream.

use std::sync::Arc;

use axum::{
    Router,
    extract::State,
    response::sse::Sse,
    routing::{get, post},
    Json,
};
use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::sse::agent_event_stream;
use crate::state::AppState;

/// POST /api/chat/send — send a message to the default agent.
async fn send_message(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SendRequest>,
) -> Result<Json<SendResponse>, AppError> {
    let handle = crate::services::agent_manager::get_default_agent(&state)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let _started = handle
        .send_message(&req.content)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(SendResponse {
        agent_id: handle.id.to_string(),
        status: handle.status().await,
    }))
}

/// GET /api/chat/stream — live SSE stream for the default agent.
///
/// Subscribes to the broadcast bus only (no history replay). The frontend
/// opens this connection once on page load and keeps it open for the
/// session, so all events produced after subscribe are delivered with no
/// gaps. Stale history from previous messages is never re-fired, which
/// avoids the terminal `done` of an earlier message closing a later one.
async fn stream_events(
    State(state): State<Arc<AppState>>,
) -> Result<
    Sse<
        impl futures::Stream<
            Item = Result<axum::response::sse::Event, std::convert::Infallible>,
        >,
    >,
    AppError,
>
where
    std::convert::Infallible: Send,
{
    let handle = crate::services::agent_manager::get_default_agent(&state)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    // Subscribe-only: no replay. Empty replay vector → live events only.
    let rx = state.event_broker.subscribe();
    Ok(agent_event_stream(handle.id, Vec::new(), rx))
}

// ── Request / Response types ──

#[derive(Debug, Deserialize)]
pub struct SendRequest {
    /// The user message content.
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct SendResponse {
    pub agent_id: String,
    pub status: String,
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/chat/send", post(send_message))
        .route("/api/chat/stream", get(stream_events))
}
