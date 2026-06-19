/// Coarse lifecycle state of an agent.
#[derive(Debug, PartialEq, Eq, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum AgentLifecycleStatus {
    IDLE,
    RUNNING,
    ABORTED,
}
