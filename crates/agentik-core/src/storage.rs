pub mod sqlite_storage;

use async_trait::async_trait;
use thiserror::Error;
use uuid::Uuid;

use crate::{lifecycle::AgentLifecycleStatus, memory::Memory};

#[derive(Debug, Clone)]
pub struct AgentSnapshot {
    pub ts: i64,
    pub agent_id: Uuid,
    pub agent_status: AgentLifecycleStatus,
    pub memory: Memory,
}

pub struct AgentBlueprint {
    pub id: String,
    pub sop: String,
    pub tool_groups: Vec<String>,
}

#[derive(Debug, Error)]
pub enum AgentSnapshotStorageError {}

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
}

#[derive(Debug, Error)]
pub enum AgentBlueprintStorageError {}
#[async_trait]
pub trait AgentBlueprintStorage {
    async fn create_blueprint(
        &self,
        blueprint: AgentBlueprint,
    ) -> Result<(), AgentBlueprintStorageError>;
    async fn get_blueprint(&self, id: String)
    -> Result<AgentBlueprint, AgentBlueprintStorageError>;
    async fn update_blueprint(
        &self,
        new_bluepirnt: AgentBlueprint,
    ) -> Result<(), AgentBlueprintStorageError>;
}
