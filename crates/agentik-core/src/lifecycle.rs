use thiserror::Error;

/// Agent lifecycle status.
///
/// Defined in [`agentik_types`] and re-exported here so historical
/// `agentik_core::lifecycle::AgentLifecycleStatus` paths keep resolving.
pub use agentik_types::AgentLifecycleStatus;

#[derive(Debug, Error)]
pub enum AgentLifecycleError {
    #[error("{0}")]
    OtherLifecycleError(String),
}

pub struct AgentLifecycle {
    status: AgentLifecycleStatus,
}

impl AgentLifecycle {
    pub fn new() -> Self {
        Self {
            status: AgentLifecycleStatus::IDLE,
        }
    }

    pub fn status(&self) -> &AgentLifecycleStatus {
        &self.status
    }

    pub fn set_idle(&mut self) {
        self.status = AgentLifecycleStatus::IDLE;
    }

    pub fn set_running(&mut self) {
        self.status = AgentLifecycleStatus::RUNNING;
    }

    pub fn set_aborted(&mut self) {
        self.status = AgentLifecycleStatus::ABORTED;
    }

    pub fn is_running(&self) -> bool {
        self.status == AgentLifecycleStatus::RUNNING
    }
}

impl Default for AgentLifecycle {
    fn default() -> Self {
        Self::new()
    }
}
