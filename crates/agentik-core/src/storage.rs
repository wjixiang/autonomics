pub mod sqlite_storage;

use async_trait::async_trait;
use thiserror::Error;
use uuid::Uuid;

use crate::{lifecycle::AgentLifecycleStatus, memory::Memory};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgentSnapshot {
    pub snapshot_id: Uuid,
    pub ts: i64,
    pub agent_id: Uuid,
    pub agent_status: AgentLifecycleStatus,
    pub memory: Memory,
}

#[derive(Debug, Error)]
pub enum AgentSnapshotStorageError {
    #[error("snapshot not found: {0}")]
    NotFound(String),
    #[error("snapshot storage error: {0}")]
    Other(#[from] Box<dyn std::error::Error + Send + Sync>),
}

#[async_trait]
pub trait AgentSnapshotStorage: Send + Sync {
    async fn create_snapshot(
        &self,
        snapshot: AgentSnapshot,
    ) -> Result<(), AgentSnapshotStorageError>;
    async fn get_snapshot(
        &self,
        snapshot_id: Uuid,
    ) -> Result<AgentSnapshot, AgentSnapshotStorageError>;
    async fn get_agent_snapshots(
        &self,
        agent_id: Uuid,
    ) -> Result<Vec<AgentSnapshot>, AgentSnapshotStorageError>;

    /// Fetch the most recent snapshot for a given agent (by timestamp).
    /// Returns `None` if no snapshots exist for the agent.
    async fn get_latest_snapshot(
        &self,
        agent_id: Uuid,
    ) -> Result<Option<AgentSnapshot>, AgentSnapshotStorageError>;

    /// List all agent IDs that have at least one snapshot.
    async fn list_all_agent_ids(&self) -> Result<Vec<Uuid>, AgentSnapshotStorageError>;

    /// Delete all snapshots for a given agent.
    async fn delete_agent_snapshots(
        &self,
        agent_id: Uuid,
    ) -> Result<usize, AgentSnapshotStorageError>;
}
