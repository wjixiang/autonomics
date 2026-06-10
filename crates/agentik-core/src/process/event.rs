use uuid::Uuid;

use agentik_types::AgentUiEvent;

use crate::lifecycle::AgentLifecycleStatus;

/// Event emitted by the [`ProcessManager`](super::ProcessManager)'s aggregated event stream.
///
/// Wraps agent-level events with the source agent's identity and adds
/// lifecycle-state-change and process-exit events that only the manager
/// can produce.
#[derive(Debug, Clone)]
pub enum ProcessEvent {
    /// An agent-level event, tagged with the source agent's ID.
    Agent {
        agent_id: Uuid,
        event: AgentUiEvent,
    },

    /// An agent's lifecycle state changed.
    StateChanged {
        agent_id: Uuid,
        new_status: AgentLifecycleStatus,
    },

    /// An agent process exited (completed, aborted, or crashed).
    ProcessExited {
        agent_id: Uuid,
        status: ProcessExitStatus,
    },
}

/// Describes how an agent process exited.
#[derive(Debug, Clone)]
pub enum ProcessExitStatus {
    /// `agent.start()` returned `Ok(())`.
    Completed,
    /// `agent.start()` returned `Err`.
    Error(String),
    /// The tokio task panicked.
    Panicked(String),
    /// Cancelled via `CancellationToken`.
    Cancelled,
    /// Explicitly stopped via a `Stop` command.
    Stopped,
}
