use std::path::Path;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use rusqlite::Connection;
use thiserror::Error;
use uuid::Uuid;

use crate::storage::{AgentSnapshot, AgentSnapshotStorage, AgentSnapshotStorageError};

/// SQLite-backed agent snapshot storage.
///
/// Writes are serialized through a `std::sync::Mutex<Connection>` and
/// dispatched via `tokio::task::spawn_blocking` so the async runtime
/// is never blocked by I/O.
pub struct SqliteAgentStorage {
    db_path: String,
    conn: Arc<Mutex<Connection>>,
}

impl SqliteAgentStorage {
    /// Open (or create) an on-disk database at `db_path`.
    ///
    /// Parent directories are created automatically. The `snapshots`
    /// table is created if it does not already exist.
    pub fn open(db_path: &str) -> Result<Self, SqliteAgentStorageError> {
        if let Some(parent) = Path::new(db_path).parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| SqliteAgentStorageError::Io(e.to_string()))?;
        }

        let conn = Connection::open(db_path)
            .map_err(SqliteAgentStorageError::from)?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS snapshots (
                snapshot_id TEXT PRIMARY KEY,
                agent_id    TEXT NOT NULL,
                ts          INTEGER NOT NULL,
                status      TEXT NOT NULL,
                memory      TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_snapshots_agent_ts
                ON snapshots(agent_id, ts DESC);",
        )
        .map_err(SqliteAgentStorageError::from)?;

        tracing::info!(%db_path, "sqlite agent storage opened");
        Ok(Self {
            db_path: db_path.to_string(),
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Open an in-memory database (useful for tests).
    pub fn in_memory() -> Result<Self, SqliteAgentStorageError> {
        let conn = Connection::open_in_memory()
            .map_err(SqliteAgentStorageError::from)?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS snapshots (
                snapshot_id TEXT PRIMARY KEY,
                agent_id    TEXT NOT NULL,
                ts          INTEGER NOT NULL,
                status      TEXT NOT NULL,
                memory      TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_snapshots_agent_ts
                ON snapshots(agent_id, ts DESC);",
        )
        .map_err(SqliteAgentStorageError::from)?;

        Ok(Self {
            db_path: ":memory:".to_string(),
            conn: Arc::new(Mutex::new(conn)),
        })
    }
}

#[derive(Debug, Error)]
pub enum SqliteAgentStorageError {
    #[error("I/O error: {0}")]
    Io(String),
    #[error("Database connection does not exist")]
    ConnectionError,
    #[error("Sqlite error: {0}")]
    SqliteError(#[from] rusqlite::Error),
    #[error("Serialization error: {0}")]
    Serialization(String),
}

/// Helper to convert `SqliteAgentStorageError` into `AgentSnapshotStorageError`.
fn to_storage_error(e: SqliteAgentStorageError) -> AgentSnapshotStorageError {
    AgentSnapshotStorageError::Other(Box::new(e))
}

// ── Helpers ───────────────────────────────────────────────────────

fn deserialize_memory(json: &str) -> Result<crate::memory::Memory, SqliteAgentStorageError> {
    serde_json::from_str(json).map_err(|e| SqliteAgentStorageError::Serialization(e.to_string()))
}

fn deserialize_status(s: &str) -> Result<crate::lifecycle::AgentLifecycleStatus, SqliteAgentStorageError> {
    serde_json::from_str(s)
        .map_err(|e| SqliteAgentStorageError::Serialization(e.to_string()))
}

/// Insert a row and return the number of rows affected.
fn insert_snapshot(
    conn: &Connection,
    snapshot: AgentSnapshot,
) -> Result<usize, SqliteAgentStorageError> {
    let memory_json = serde_json::to_string(&snapshot.memory)
        .map_err(|e| SqliteAgentStorageError::Serialization(e.to_string()))?;
    let status_json = serde_json::to_string(&snapshot.agent_status)
        .map_err(|e| SqliteAgentStorageError::Serialization(e.to_string()))?;

    conn.execute(
        "INSERT INTO snapshots (snapshot_id, agent_id, ts, status, memory) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![
            snapshot.snapshot_id.to_string(),
            snapshot.agent_id.to_string(),
            snapshot.ts,
            status_json,
            memory_json,
        ],
    )
    .map_err(SqliteAgentStorageError::from)
}

/// Query a single snapshot by its id.
fn query_snapshot(conn: &Connection, snapshot_id: &str) -> Result<Option<AgentSnapshot>, SqliteAgentStorageError> {
    let mut stmt = conn
        .prepare(
            "SELECT snapshot_id, agent_id, ts, status, memory FROM snapshots WHERE snapshot_id = ?1",
        )
        .map_err(SqliteAgentStorageError::from)?;

    let mut rows = stmt.query(rusqlite::params![snapshot_id])
        .map_err(SqliteAgentStorageError::from)?;

    match rows.next() {
        Ok(Some(row)) => Ok(Some(row_to_snapshot(row)?)),
        Ok(None) => Ok(None),
        Err(e) => Err(SqliteAgentStorageError::from(e)),
    }
}

/// Query all snapshots for an agent, ordered by timestamp descending.
fn query_agent_snapshots(conn: &Connection, agent_id: &str) -> Result<Vec<AgentSnapshot>, SqliteAgentStorageError> {
    let mut stmt = conn
        .prepare(
            "SELECT snapshot_id, agent_id, ts, status, memory FROM snapshots WHERE agent_id = ?1 ORDER BY ts DESC",
        )
        .map_err(SqliteAgentStorageError::from)?;

    let rows = stmt
        .query_map(rusqlite::params![agent_id], |row| {
            let snapshot_id: String = row.get(0)?;
            let agent_id: String = row.get(1)?;
            let ts: i64 = row.get(2)?;
            let status_json: String = row.get(3)?;
            let memory_json: String = row.get(4)?;
            Ok((snapshot_id, agent_id, ts, status_json, memory_json))
        })
        .map_err(SqliteAgentStorageError::from)?;

    let mut snapshots = Vec::new();
    for row in rows {
        let (snapshot_id, agent_id, ts, status_json, memory_json) =
            row.map_err(SqliteAgentStorageError::from)?;
        snapshots.push(AgentSnapshot {
            snapshot_id: Uuid::parse_str(&snapshot_id)
                .map_err(|e| SqliteAgentStorageError::Serialization(e.to_string()))?,
            agent_id: Uuid::parse_str(&agent_id)
                .map_err(|e| SqliteAgentStorageError::Serialization(e.to_string()))?,
            ts,
            agent_status: deserialize_status(&status_json)?,
            memory: deserialize_memory(&memory_json)?,
        });
    }
    Ok(snapshots)
}

/// Query the latest snapshot for an agent.
fn query_latest_snapshot(conn: &Connection, agent_id: &str) -> Result<Option<AgentSnapshot>, SqliteAgentStorageError> {
    let mut stmt = conn
        .prepare(
            "SELECT snapshot_id, agent_id, ts, status, memory FROM snapshots WHERE agent_id = ?1 ORDER BY ts DESC LIMIT 1",
        )
        .map_err(SqliteAgentStorageError::from)?;

    let mut rows = stmt.query(rusqlite::params![agent_id])
        .map_err(SqliteAgentStorageError::from)?;

    match rows.next() {
        Ok(Some(row)) => Ok(Some(row_to_snapshot(row)?)),
        Ok(None) => Ok(None),
        Err(e) => Err(SqliteAgentStorageError::from(e)),
    }
}

fn row_to_snapshot(row: &rusqlite::Row<'_>) -> Result<AgentSnapshot, SqliteAgentStorageError> {
    let snapshot_id: String = row.get(0)?;
    let agent_id: String = row.get(1)?;
    let ts: i64 = row.get(2)?;
    let status_json: String = row.get(3)?;
    let memory_json: String = row.get(4)?;
    Ok(AgentSnapshot {
        snapshot_id: Uuid::parse_str(&snapshot_id)
            .map_err(|e| SqliteAgentStorageError::Serialization(e.to_string()))?,
        agent_id: Uuid::parse_str(&agent_id)
            .map_err(|e| SqliteAgentStorageError::Serialization(e.to_string()))?,
        ts,
        agent_status: deserialize_status(&status_json)?,
        memory: deserialize_memory(&memory_json)?,
    })
}

fn query_all_agent_ids(conn: &Connection) -> Result<Vec<Uuid>, SqliteAgentStorageError> {
    let mut stmt = conn
        .prepare("SELECT DISTINCT agent_id FROM snapshots ORDER BY agent_id")
        .map_err(SqliteAgentStorageError::from)?;

    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(SqliteAgentStorageError::from)?;

    let mut ids = Vec::new();
    for row in rows {
        let id_str: String = row.map_err(SqliteAgentStorageError::from)?;
        ids.push(
            Uuid::parse_str(&id_str)
                .map_err(|e| SqliteAgentStorageError::Serialization(e.to_string()))?,
        );
    }
    Ok(ids)
}

fn delete_snapshots_by_agent(conn: &Connection, agent_id: &str) -> Result<usize, SqliteAgentStorageError> {
    let count = conn
        .execute("DELETE FROM snapshots WHERE agent_id = ?1", rusqlite::params![agent_id])
        .map_err(SqliteAgentStorageError::from)?;
    Ok(count)
}

#[async_trait]
impl AgentSnapshotStorage for SqliteAgentStorage {
    async fn create_snapshot(
        &self,
        snapshot: AgentSnapshot,
    ) -> Result<(), AgentSnapshotStorageError> {
        let conn = Arc::clone(&self.conn);
        tokio::task::spawn_blocking(move || {
            let guard = conn.lock().unwrap();
            insert_snapshot(&guard, snapshot).map_err(to_storage_error)?;
            Ok(())
        })
        .await
        .expect("spawn_blocking task panicked")
    }

    async fn get_snapshot(
        &self,
        snapshot_id: Uuid,
    ) -> Result<AgentSnapshot, AgentSnapshotStorageError> {
        let conn = Arc::clone(&self.conn);
        let id_str = snapshot_id.to_string();
        tokio::task::spawn_blocking(move || {
            let guard = conn.lock().unwrap();
            query_snapshot(&guard, &id_str)
                .map_err(to_storage_error)?
                .ok_or_else(|| AgentSnapshotStorageError::NotFound(format!("snapshot {id_str} not found")))
        })
        .await
        .expect("spawn_blocking task panicked")
    }

    async fn get_agent_snapshots(
        &self,
        agent_id: Uuid,
    ) -> Result<Vec<AgentSnapshot>, AgentSnapshotStorageError> {
        let conn = Arc::clone(&self.conn);
        let id_str = agent_id.to_string();
        tokio::task::spawn_blocking(move || {
            let guard = conn.lock().unwrap();
            query_agent_snapshots(&guard, &id_str).map_err(to_storage_error)
        })
        .await
        .expect("spawn_blocking task panicked")
    }

    async fn get_latest_snapshot(
        &self,
        agent_id: Uuid,
    ) -> Result<Option<AgentSnapshot>, AgentSnapshotStorageError> {
        let conn = Arc::clone(&self.conn);
        let id_str = agent_id.to_string();
        tokio::task::spawn_blocking(move || {
            let guard = conn.lock().unwrap();
            query_latest_snapshot(&guard, &id_str).map_err(to_storage_error)
        })
        .await
        .expect("spawn_blocking task panicked")
    }

    async fn list_all_agent_ids(&self) -> Result<Vec<Uuid>, AgentSnapshotStorageError> {
        let conn = Arc::clone(&self.conn);
        tokio::task::spawn_blocking(move || {
            let guard = conn.lock().unwrap();
            query_all_agent_ids(&guard).map_err(to_storage_error)
        })
        .await
        .expect("spawn_blocking task panicked")
    }

    async fn delete_agent_snapshots(
        &self,
        agent_id: Uuid,
    ) -> Result<usize, AgentSnapshotStorageError> {
        let conn = Arc::clone(&self.conn);
        let id_str = agent_id.to_string();
        tokio::task::spawn_blocking(move || {
            let guard = conn.lock().unwrap();
            delete_snapshots_by_agent(&guard, &id_str).map_err(to_storage_error)
        })
        .await
        .expect("spawn_blocking task panicked")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message_ext::AgentMessageExt;
    use crate::memory::{Memory, MemoryItem};
    use crate::lifecycle::AgentLifecycleStatus;

    #[tokio::test]
    async fn test_create_and_get_snapshot() {
        let store = SqliteAgentStorage::in_memory().unwrap();

        let memory = Memory { items: vec![
            MemoryItem {
                messages: vec![agentik_sdk::types::messages::Message::user("hello")],
                summary: None,
            },
        ]};

        let snapshot = AgentSnapshot {
            snapshot_id: uuid::Uuid::new_v4(),
            ts: 1000,
            agent_id: uuid::Uuid::new_v4(),
            agent_status: AgentLifecycleStatus::IDLE,
            memory: memory.clone(),
        };

        store.create_snapshot(snapshot.clone()).await.unwrap();

        let fetched = store.get_snapshot(snapshot.snapshot_id).await.unwrap();
        assert_eq!(fetched.snapshot_id, snapshot.snapshot_id);
        assert_eq!(fetched.ts, 1000);
        assert_eq!(fetched.agent_status, AgentLifecycleStatus::IDLE);
    }

    #[tokio::test]
    async fn test_get_latest_snapshot() {
        let store = SqliteAgentStorage::in_memory().unwrap();
        let agent_id = uuid::Uuid::new_v4();

        let memory = Memory::new();

        // First snapshot
        let snap1 = AgentSnapshot {
            snapshot_id: uuid::Uuid::new_v4(),
            ts: 1000,
            agent_id,
            agent_status: AgentLifecycleStatus::RUNNING,
            memory: memory.clone(),
        };
        store.create_snapshot(snap1).await.unwrap();

        // Second snapshot (later)
        let snap2 = AgentSnapshot {
            snapshot_id: uuid::Uuid::new_v4(),
            ts: 2000,
            agent_id,
            agent_status: AgentLifecycleStatus::IDLE,
            memory,
        };
        store.create_snapshot(snap2.clone()).await.unwrap();

        let latest = store.get_latest_snapshot(agent_id).await.unwrap();
        assert!(latest.is_some());
        let latest = latest.unwrap();
        assert_eq!(latest.snapshot_id, snap2.snapshot_id);
        assert_eq!(latest.ts, 2000);
        assert_eq!(latest.agent_status, AgentLifecycleStatus::IDLE);
    }

    #[tokio::test]
    async fn test_get_agent_snapshots_ordering() {
        let store = SqliteAgentStorage::in_memory().unwrap();
        let agent_id = uuid::Uuid::new_v4();
        let other_agent = uuid::Uuid::new_v4();

        let memory = Memory::new();

        store.create_snapshot(AgentSnapshot {
            snapshot_id: uuid::Uuid::new_v4(),
            ts: 3000,
            agent_id,
            agent_status: AgentLifecycleStatus::IDLE,
            memory: memory.clone(),
        }).await.unwrap();

        store.create_snapshot(AgentSnapshot {
            snapshot_id: uuid::Uuid::new_v4(),
            ts: 1000,
            agent_id,
            agent_status: AgentLifecycleStatus::RUNNING,
            memory: memory.clone(),
        }).await.unwrap();

        // Different agent — should not appear
        store.create_snapshot(AgentSnapshot {
            snapshot_id: uuid::Uuid::new_v4(),
            ts: 2000,
            agent_id: other_agent,
            agent_status: AgentLifecycleStatus::IDLE,
            memory,
        }).await.unwrap();

        let snapshots = store.get_agent_snapshots(agent_id).await.unwrap();
        assert_eq!(snapshots.len(), 2);
        // Ordered by ts DESC
        assert_eq!(snapshots[0].ts, 3000);
        assert_eq!(snapshots[1].ts, 1000);
    }

    #[tokio::test]
    async fn test_latest_snapshot_none_for_empty() {
        let store = SqliteAgentStorage::in_memory().unwrap();
        let result = store.get_latest_snapshot(uuid::Uuid::new_v4()).await.unwrap();
        assert!(result.is_none());
    }
}
