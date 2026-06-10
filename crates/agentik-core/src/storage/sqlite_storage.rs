use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use rusqlite::Connection;
use thiserror::Error;
use uuid::Uuid;

use crate::storage::{AgentSnapshot, AgentSnapshotStorage, AgentSnapshotStorageError};

pub struct SqliteAgentStorage {
    db_path: String,
    db_name: String,
    conn: Option<Arc<Mutex<Connection>>>,
}
impl Default for SqliteAgentStorage {
    fn default() -> Self {
        Self {
            conn: None,
            db_path: "./db/agent_db.db3".to_string(),
            db_name: "agent_db".to_string(),
        }
    }
}

impl SqliteAgentStorage {
    pub fn in_memory(mut self) -> Result<Self, rusqlite::Error> {
        self.conn = Some(Arc::new(Mutex::new(Connection::open_in_memory()?)));
        Ok(self)
    }

    pub fn in_disk(mut self, db_path: Option<&str>) -> Result<Self, rusqlite::Error> {
        if let Some(path) = db_path {
            self.db_path = path.to_string();
        }

        self.conn = Some(Arc::new(Mutex::new(Connection::open(&self.db_path)?)));
        Ok(self)
    }

    pub fn init(&self) -> Result<(), SqliteAgentStorageError> {
        // Create Snapshot table
        let guard = self
            .conn
            .as_ref()
            .ok_or(SqliteAgentStorageError::ConnectionError)?
            .lock()
            .unwrap(); // When poison error happen, panic the process directly
        let exists = guard.table_exists(Some(self.db_name.as_str()), self.db_name.as_str())?;
        todo!()
    }
}

#[derive(Debug, Error)]
pub enum SqliteAgentStorageError {
    #[error("Database connection does not exist")]
    ConnectionError,

    #[error("Sqlite error")]
    SqliteError(#[from] rusqlite::Error),
}

#[async_trait]
impl AgentSnapshotStorage for SqliteAgentStorage {
    async fn create_snapshot(
        &self,
        snapshot: AgentSnapshot,
    ) -> Result<(), AgentSnapshotStorageError> {
        todo!()
    }

    async fn get_snapshot(
        &self,
        snapshot_id: Uuid,
    ) -> Result<AgentSnapshot, AgentSnapshotStorageError> {
        todo!()
    }

    async fn get_agent_snapshots(
        &self,
        agent_id: Uuid,
    ) -> Result<Vec<AgentSnapshot>, AgentSnapshotStorageError> {
        todo!()
    }
}
