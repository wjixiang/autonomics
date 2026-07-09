//! Internal events for decoupled communication between subsystems and the
//! main TUI loop. Modeled after codex's `AppEvent` + `AppEventSender` pattern.
//!
//! Components that don't have direct access to the `App` struct (e.g. future
//! file search, plugin watchers, background tasks) can push events through
//! an `AppEventSender` which the main loop drains each tick.

/// Events that can be processed by the main TUI loop.
#[derive(Debug)]
pub(crate) enum AppEvent {
    /// An event from the agent runtime (text delta, tool call, etc.).
    Agent(agentik_sdk::types::AgentEvent),
    /// Request to exit the application.
    Quit,
    /// Config data changed; the Config tab should reload from the database.
    ConfigReload,
}
