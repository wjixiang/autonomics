//! State types and state-machine methods for the Agent Status Panel.
//!
//! The data model is plain-old-data on purpose: the panel never borrows
//! the host's `App` or any other context, so the host (or a future
//! test) can own a `AgentPanelState` without any setup beyond
//! `default()`. Side-effects (caching, animation) live in the renderer.

use std::time::{Duration, Instant};

use agentik_runtime::process::ProcessEvent;
use agentik_runtime::AgentEvent;
use serde_json::Value;

use crate::events::{apply_agent_event, map_agent_event};
use crate::tools::{tool_user_facing_name, AgentPanelTools};

/// Layout hints passed from the per-frame sort/prioritization step
/// to the per-row renderer. Carries enough context for the row
/// renderer to choose icons, styles, and connector characters
/// without re-walking the global state.
#[derive(Debug, Clone, Copy)]
pub struct AgentEntryLayout {
    pub index_1based: usize,
    pub total: usize,
    pub is_last: bool,
    pub is_selected: bool,
    pub now: Instant,
}

/// Maximum number of expanded events retained per agent row. Older
/// events are dropped from the front as new ones arrive. The cap is
/// deliberately tight to keep a long-running agent from blowing up
/// memory if the user leaves its row expanded.
pub(crate) const MAX_EVENTS_PER_AGENT: usize = 20;

/// Cap on how many agent rows the renderer tries to show at once.
/// Anything beyond this is folded into a "… +N completed" footer.
pub const MAX_VISIBLE_AGENTS: usize = 8;

/// How long (in ms) a completed agent row stays "highlighted" before
/// it visually fades. Doubles as the TTL for the visible "recent
/// completion" hint.
pub const RECENT_COMPLETED_TTL_MS: u64 = 30_000;

/// Sub-agent status list state. Owned by the host (`App::agent_panel`)
/// and mutated through the `apply_*` and `add_agent` methods.
#[derive(Debug, Clone, Default)]
pub struct AgentPanelState {
    pub agents: Vec<AgentPanelEntry>,
    pub selected: usize,
}

/// One row in the sub-agent status list. Public fields because the
/// renderer reads them directly; the host (or a future test) can
/// inspect or assert on them without going through a getter.
#[derive(Debug, Clone)]
pub struct AgentPanelEntry {
    pub agent_id: uuid::Uuid,
    pub title: String,
    pub status: AgentEntryStatus,
    pub events: Vec<AgentPanelEvent>,
    pub expanded: bool,
    pub started_at: Instant,
    pub completed_at: Option<Instant>,
    pub tool_call_count: usize,
    pub streaming_text: Option<String>,
}

#[derive(Debug, Clone)]
pub enum AgentEntryStatus {
    Running,
    Completed { duration: Duration },
    Failed { error: String, duration: Duration },
}

/// One event in the per-agent expandable log. We keep our own enum
/// (rather than `AgentEvent` directly) so the panel can dedup noisy
/// variants like `TextDelta` and render `ToolResult` in a host-defined
/// way without reaching back into the agent layer.
#[derive(Debug, Clone)]
pub enum AgentPanelEvent {
    LlmResponse(String),
    ToolCall { name: String, input: Value },
    ToolResult { ok: bool, content: String },
    Error(String),
}

impl AgentPanelState {
    /// Apply a `ProcessEvent` from `ProcessManager` to the panel state.
    pub fn apply_process_event(&mut self, event: &ProcessEvent) {
        match event {
            ProcessEvent::Agent {
                agent_id,
                event: ui_event,
            } => {
                if let Some(entry) = self.agents.iter_mut().find(|e| e.agent_id == *agent_id) {
                    apply_agent_event(entry, ui_event);
                }
            }
            ProcessEvent::StateChanged { agent_id, .. } => {
                // Register new agents we haven't seen yet.
                let known = self.agents.iter().any(|e| e.agent_id == *agent_id);
                if !known {
                    // Will be given a proper title by the caller
                    // (from `agent_titles` map). Use UUID prefix as
                    // fallback.
                    let title = format!("Agent {}", &agent_id.to_string()[..8]);
                    self.add_agent(*agent_id, title);
                }
            }
            ProcessEvent::ProcessExited { agent_id, status } => {
                if let Some(entry) = self.agents.iter_mut().find(|e| e.agent_id == *agent_id) {
                    let duration = entry.started_at.elapsed();
                    match status {
                        agentik_runtime::process::ProcessExitStatus::Completed => {
                            entry.status = AgentEntryStatus::Completed { duration };
                        }
                        agentik_runtime::process::ProcessExitStatus::Error(msg)
                        | agentik_runtime::process::ProcessExitStatus::Panicked(msg) => {
                            entry.status = AgentEntryStatus::Failed {
                                error: msg.clone(),
                                duration,
                            };
                        }
                        agentik_runtime::process::ProcessExitStatus::Cancelled
                        | agentik_runtime::process::ProcessExitStatus::Stopped => {
                            entry.status = AgentEntryStatus::Completed { duration };
                        }
                    }
                    entry.completed_at = Some(Instant::now());
                }
            }
        }
    }

    /// Register a new agent (or update the title of an existing one).
    /// The host typically calls this when it first sees a new
    /// `ProcessEvent::StateChanged` and wants to attach a human-readable
    /// title from its own `agent_titles` map.
    pub fn add_agent(&mut self, agent_id: uuid::Uuid, title: String) {
        // Don't duplicate.
        if self.agents.iter().any(|e| e.agent_id == agent_id) {
            // Update the title if we now have a better one.
            if let Some(e) = self.agents.iter_mut().find(|e| e.agent_id == agent_id)
                && !title.starts_with("Agent ")
            {
                e.title = title;
            }
            return;
        }
        self.agents.push(AgentPanelEntry {
            agent_id,
            title,
            status: AgentEntryStatus::Running,
            events: Vec::new(),
            expanded: false,
            started_at: Instant::now(),
            completed_at: None,
            tool_call_count: 0,
            streaming_text: None,
        });
    }

    pub fn running_count(&self) -> usize {
        self.agents
            .iter()
            .filter(|e| matches!(e.status, AgentEntryStatus::Running))
            .count()
    }

    pub fn completed_count(&self) -> usize {
        self.agents
            .iter()
            .filter(|e| matches!(e.status, AgentEntryStatus::Completed { .. }))
            .count()
    }

    pub fn failed_count(&self) -> usize {
        self.agents
            .iter()
            .filter(|e| matches!(e.status, AgentEntryStatus::Failed { .. }))
            .count()
    }

    pub fn move_selection(&mut self, delta: isize) {
        if self.agents.is_empty() {
            return;
        }
        let len = self.agents.len() as isize;
        let cur = self.selected as isize;
        let next = (cur + delta).clamp(0, len - 1);
        self.selected = next as usize;
    }

    pub fn toggle_selected(&mut self) {
        if let Some(entry) = self.agents.get_mut(self.selected) {
            entry.expanded = !entry.expanded;
        }
    }

    pub fn expand_all(&mut self) {
        for e in self.agents.iter_mut() {
            e.expanded = true;
        }
    }

    pub fn collapse_all(&mut self) {
        for e in self.agents.iter_mut() {
            e.expanded = false;
        }
    }

    /// Remove completed/failed agents older than the TTL.
    #[allow(dead_code)]
    pub fn prune_old(&mut self) {
        let now = Instant::now();
        self.agents.retain(|e| {
            !matches!(
                e.completed_at,
                Some(t) if now.duration_since(t).as_millis() > RECENT_COMPLETED_TTL_MS as u128 * 3
            )
        });
        // Keep selected in bounds.
        if !self.agents.is_empty() && self.selected >= self.agents.len() {
            self.selected = self.agents.len() - 1;
        }
    }
}

// ---- Per-entry helpers (migrated from parallel_panel.rs) -------------------

impl AgentPanelEntry {
    pub fn elapsed(&self) -> Duration {
        match &self.status {
            AgentEntryStatus::Running => Duration::ZERO,
            AgentEntryStatus::Completed { duration }
            | AgentEntryStatus::Failed { duration, .. } => *duration,
        }
    }

    pub fn is_recently_completed(&self, now: Instant) -> bool {
        matches!(
            self.completed_at,
            Some(t) if now.duration_since(t).as_millis() < RECENT_COMPLETED_TTL_MS as u128
        )
    }

    pub fn activity_hint(&self) -> Option<String> {
        self.activity_hint_with(&DefaultToolsBridge)
    }

    /// Like [`Self::activity_hint`] but lets the caller pass a
    /// `&dyn AgentPanelTools` so the rendered label can come from
    /// the host's own renderer instead of the built-in KMS list.
    pub fn activity_hint_with(&self, tools: &dyn AgentPanelTools) -> Option<String> {
        if matches!(self.status, AgentEntryStatus::Failed { .. }) {
            return None;
        }
        if let Some(text) = &self.streaming_text
            && !text.is_empty()
        {
            let char_count = text.chars().count();
            let snippet: String = if char_count > 60 {
                format!(
                    "...{}",
                    text.chars().skip(char_count - 60).collect::<String>()
                )
            } else {
                text.clone()
            };
            return Some(snippet);
        }
        for ev in self.events.iter().rev() {
            match ev {
                AgentPanelEvent::ToolCall { name, input } => {
                    return Some(tools.user_facing_name(name, input));
                }
                AgentPanelEvent::ToolResult { ok: true, .. } => {
                    return Some("done".to_string());
                }
                AgentPanelEvent::ToolResult { ok: false, content } => {
                    let snippet = content
                        .lines()
                        .next()
                        .unwrap_or("")
                        .chars()
                        .take(40)
                        .collect::<String>();
                    return Some(if snippet.is_empty() {
                        "tool failed".to_string()
                    } else {
                        format!("tool failed: {}", snippet)
                    });
                }
                AgentPanelEvent::Error(msg) => {
                    let snippet = msg.chars().take(40).collect::<String>();
                    return Some(if snippet.is_empty() {
                        "error".to_string()
                    } else {
                        format!("error: {}", snippet)
                    });
                }
                AgentPanelEvent::LlmResponse(s) if !s.is_empty() => {
                    let snippet = s.chars().take(40).collect::<String>();
                    return Some(if s.chars().count() > 40 {
                        format!("{}…", snippet)
                    } else {
                        snippet
                    });
                }
                AgentPanelEvent::LlmResponse(_) => continue,
            }
        }
        None
    }
}

// Silence the dead-code warning for `map_agent_event` while it stays
// here as a future extension point (some events that we currently
// ignore may need a panel-side projection later).
#[allow(dead_code)]
pub(crate) fn _hint(event: &AgentEvent) -> Option<AgentPanelEvent> {
    map_agent_event(event)
}

/// Zero-sized bridge used by `activity_hint()` (the no-arg variant)
/// to keep the same body as `activity_hint_with`. Routes through the
/// default tool-name renderer.
struct DefaultToolsBridge;

impl AgentPanelTools for DefaultToolsBridge {
    fn user_facing_name(&self, name: &str, input: &serde_json::Value) -> String {
        tool_user_facing_name(name, input)
    }
}
