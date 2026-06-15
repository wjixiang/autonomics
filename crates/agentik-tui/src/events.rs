//! Translation from `ProcessEvent` / `AgentEvent` into the panel's
//! internal `AgentPanelEvent` log.

use agentik_runtime::AgentEvent;
use serde_json::Value;

use crate::state::{AgentPanelEntry, AgentPanelEvent, MAX_EVENTS_PER_AGENT};

pub(crate) fn apply_agent_event(entry: &mut AgentPanelEntry, event: &AgentEvent) {
    match event {
        AgentEvent::ToolCall { .. } => {
            entry.tool_call_count += 1;
            if let Some(mapped) = map_agent_event(event) {
                entry.events.push(mapped);
            }
        }
        AgentEvent::TextDelta(token) => {
            entry
                .streaming_text
                .get_or_insert_with(String::new)
                .push_str(token);
        }
        AgentEvent::LlmResponse(text) => {
            entry.streaming_text = None;
            if !text.is_empty() {
                entry
                    .events
                    .push(AgentPanelEvent::LlmResponse(text.clone()));
            }
        }
        _ => {
            if let Some(mapped) = map_agent_event(event) {
                entry.events.push(mapped);
            }
        }
    }
    if entry.events.len() > MAX_EVENTS_PER_AGENT {
        let drop_n = entry.events.len() - MAX_EVENTS_PER_AGENT;
        entry.events.drain(0..drop_n);
    }
}

pub(crate) fn map_agent_event(event: &AgentEvent) -> Option<AgentPanelEvent> {
    match event {
        AgentEvent::LlmResponse(text) => Some(AgentPanelEvent::LlmResponse(text.clone())),
        AgentEvent::ToolCall { name, input } => Some(AgentPanelEvent::ToolCall {
            name: name.clone(),
            input: input.clone(),
        }),
        AgentEvent::ToolResult { ok, content } => Some(AgentPanelEvent::ToolResult {
            ok: *ok,
            content: content.clone(),
        }),
        AgentEvent::Error(msg) => Some(AgentPanelEvent::Error(msg.clone())),
        AgentEvent::Thinking(_) => None,
        AgentEvent::Requesting | AgentEvent::Done => None,
        AgentEvent::TextDelta(_)
        | AgentEvent::ThinkingDelta(_)
        | AgentEvent::UsageUpdate { .. }
        | AgentEvent::StreamStart { .. }
        | AgentEvent::ContentBlockStart { .. }
        | AgentEvent::ContentBlockStop { .. }
        | AgentEvent::StreamDelta { .. } => None,
    }
}

// Avoid `Value` unused-import warning on this module (the type is
// re-exported in `state.rs` via `AgentPanelEvent::ToolCall`).
#[allow(dead_code)]
fn _import_marker(_: Value) {}
