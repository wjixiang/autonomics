use thiserror::Error;
use uuid::Uuid;

use crate::error::AgentError;

/// Errors produced by the [`ProcessManager`](super::ProcessManager).
#[derive(Debug, Error)]
pub enum ProcessError {
    #[error("agent process '{0}' not found")]
    NotFound(Uuid),

    #[error("agent '{agent_id}' is already {status}")]
    InvalidState {
        agent_id: Uuid,
        status: String,
    },

    #[error("agent '{agent_id}' failed: {source}")]
    AgentFailed {
        agent_id: Uuid,
        #[source]
        source: AgentError,
    },

    #[error("agent '{agent_id}' task panicked: {message}")]
    AgentPanicked {
        agent_id: Uuid,
        message: String,
    },

    #[error("command channel closed for agent '{0}'")]
    ChannelClosed(Uuid),

    #[error("manager already shut down")]
    Shutdown,
}
