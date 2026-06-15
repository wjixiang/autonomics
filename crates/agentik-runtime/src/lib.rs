//! Multi-agent runtime layer for `agentik`.
//!
//! Hosts the [`process`] module — a [`ProcessManager`](process::ProcessManager) that
//! spawns, monitors, and controls multiple [`Agent`](agentik_core::Agent) instances as
//! independent tokio tasks.

pub mod process;

pub use process::{ProcessError, ProcessEvent, ProcessExitStatus, ProcessManager};

// Re-export AgentEvent so downstream TUI crates can use it without
// depending on agentik-sdk directly.
pub use agentik_sdk::types::AgentEvent;
