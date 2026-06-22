//! SSE helper: converts tokio broadcast stream into axum SSE response.

use std::convert::Infallible;

use axum::response::sse::{Event, KeepAlive, Sse};
use futures::StreamExt;
use tokio::sync::broadcast;
use uuid::Uuid;

use agentik_sdk::types::AgentEvent;

/// Convert an `AgentEvent` into an event-type string and a JSON value.
/// Shared between SSE rendering and webhook payload serialization.
pub(crate) fn event_to_parts(event: &AgentEvent) -> (&'static str, serde_json::Value) {
    match event {
        AgentEvent::TextDelta(text) => ("text_delta", serde_json::json!(text)),
        AgentEvent::ThinkingDelta(text) => ("thinking_delta", serde_json::json!(text)),
        AgentEvent::UsageUpdate {
            input_tokens,
            output_tokens,
        } => (
            "usage_update",
            serde_json::json!({ "input_tokens": input_tokens, "output_tokens": output_tokens }),
        ),
        AgentEvent::StreamStart { message } => (
            "stream_start",
            serde_json::to_value(message).unwrap_or(serde_json::Value::Null),
        ),
        AgentEvent::ContentBlockStart { index, content_block_kind } => (
            "content_block_start",
            serde_json::json!({ "index": index, "kind": content_block_kind }),
        ),
        AgentEvent::ContentBlockStop { index } => (
            "content_block_stop",
            serde_json::json!({ "index": index }),
        ),
        AgentEvent::StreamDelta { stop_reason } => (
            "stream_delta",
            serde_json::json!({ "stop_reason": stop_reason }),
        ),
        AgentEvent::LlmResponse(text) => ("llm_response", serde_json::json!(text)),
        AgentEvent::Thinking(text) => ("thinking", serde_json::json!(text)),
        AgentEvent::Requesting => ("requesting", serde_json::Value::Null),
        AgentEvent::ToolCall { name, input } => (
            "tool_call",
            serde_json::json!({ "name": name, "input": input }),
        ),
        AgentEvent::ToolResult { ok, content } => (
            "tool_result",
            serde_json::json!({ "ok": ok, "content": content }),
        ),
        AgentEvent::Done => ("done", serde_json::Value::Null),
        AgentEvent::Error(msg) => ("error", serde_json::json!(msg)),
    }
}

/// Render a single `AgentEvent` as an SSE frame.
fn render_event(event: &AgentEvent) -> Event {
    let (event_type, data) = event_to_parts(event);
    // NOTE: axum's `Event::data("")` is a no-op for empty strings — its
    // internal writer short-circuits on empty input and never emits a
    // `data:` line. Per the SSE spec a browser does not dispatch an event
    // block that has no data field, so `done`/`requesting` (whose payload
    // is `Value::Null`) silently vanished and the frontend never saw them.
    // Emit a non-empty placeholder so the frame always carries a data line.
    let data_str = match data {
        serde_json::Value::String(s) => s,
        serde_json::Value::Null => String::from("null"),
        other => other.to_string(),
    };
    Event::default().event(event_type).data(data_str)
}

/// Create an SSE body stream that yields events for a specific agent.
///
/// First replays the buffered `replay` events (captured atomically with the
/// broadcast subscribe by `AgentHandle::attach`), then continues with the
/// live broadcast bus filtered by `agent_id`. This guarantees a late client
/// sees the full event history — including a terminal `Done`/`Error` already
/// emitted before it connected — with no gaps or duplicates.
pub fn agent_event_stream(
    agent_id: Uuid,
    replay: Vec<AgentEvent>,
    rx: broadcast::Receiver<(Uuid, AgentEvent)>,
) -> Sse<impl futures::Stream<Item = Result<Event, Infallible>>> {
    // Replay phase: render the buffered history as one frame each.
    let replay_stream =
        futures::stream::iter(replay.into_iter().map(|event| Ok(render_event(&event))));

    // Live phase: filter the broadcast bus by agent_id.
    let live_stream = tokio_stream::wrappers::BroadcastStream::new(rx)
        .filter_map(move |result| async move {
            let Ok((id, event)) = result else {
                return None;
            };
            if id != agent_id {
                return None;
            }
            Some(Ok(render_event(&event)))
        });

    Sse::new(replay_stream.chain(live_stream)).keep_alive(KeepAlive::default())
}
