use thiserror::Error;

#[derive(Debug, Error)]
pub enum AgentLifecycleError {
    #[error("{0}")]
    OtherLifecycleError(String),
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum AgentLifecycleStatus {
    IDLE,
    RUNNING,
    ABORTED,
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
