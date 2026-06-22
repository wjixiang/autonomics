//! Agent pool management: spawn, track, and bridge events to the global bus.

use std::sync::Arc;

use tokio::sync::mpsc;
use uuid::Uuid;

use agentik_core::Agent;
use agentik_sdk::types::{AgentEvent, ContentBlock};
use datalake_tools::iceberg_registrations;

use crate::state::AppState;

/// A running agent instance tracked by the pool.
#[derive(Clone)]
pub struct AgentHandle {
    pub id: Uuid,
    pub identity: String,
    /// The agent itself. Wrapped in Mutex for interior mutability.
    agent: Arc<tokio::sync::Mutex<Agent>>,
    /// Append-only event log for this agent.
    ///
    /// The bridge task pushes every event here **and** to the global
    /// broadcast bus under the same lock. Late SSE subscribers snapshot
    /// this buffer (plus the buffered terminal `Done`/`Error`) before
    /// attaching to the live broadcast, so events produced between the
    /// `POST /api/chat` spawn and the `GET .../stream` subscribe are not
    /// lost — closing the race where a fast/failed agent run produced no
    /// visible response.
    events: Arc<tokio::sync::Mutex<Vec<AgentEvent>>>,
}

impl AgentHandle {
    /// Get the agent's current lifecycle status.
    pub async fn status(&self) -> String {
        let agent = self.agent.lock().await;
        format!("{:?}", agent.lifecycle_status())
    }

    /// Atomically snapshot the replay log **and** subscribe to the live
    /// broadcast bus.
    ///
    /// Both happen while holding the log lock. Because the bridge appends
    /// to the log and broadcasts under that same lock, no event can slip
    /// into the gap between the snapshot and the subscribe: any event not
    /// in the returned `replay` has not yet been broadcast, so the live
    /// receiver will pick it up — no missed events, no duplicates.
    pub async fn attach(
        &self,
        broker: &tokio::sync::broadcast::Sender<(Uuid, AgentEvent)>,
    ) -> (
        Vec<AgentEvent>,
        tokio::sync::broadcast::Receiver<(Uuid, AgentEvent)>,
    ) {
        let _log = self.events.lock().await;
        let replay = (*_log).clone();
        tracing::debug!(agent_id = %self.id, replay_len = replay.len(), "attach: replay snapshot");
        let rx = broker.subscribe();
        (replay, rx)
    }

    /// Inject a user message and start the agent loop if not already running.
    ///
    /// Returns `true` if a new run was started, `false` if the agent was
    /// already running (the message is queued in memory and will be picked
    /// up by the existing loop).
    pub async fn send_message(&self, content: &str) -> anyhow::Result<bool> {
        let agent = self.agent.clone();
        let mut guard = agent.lock().await;
        let user_content = vec![ContentBlock::Text {
            text: content.to_string(),
        }];
        guard.inject_message(user_content)?;

        let need_start = !guard.is_running();
        drop(guard);

        if need_start {
            let agent_clone = Arc::clone(&self.agent);
            let id = self.id;
            tracing::info!(agent_id = %id, "agent is idle, spawning run loop");
            tokio::spawn(async move {
                let mut a = agent_clone.lock().await;
                tracing::info!(agent_id = %id, "agent run loop started");
                match a.start().await {
                    Ok(()) => tracing::info!(agent_id = %id, "agent run loop completed"),
                    Err(e) => tracing::error!(agent_id = %id, error = %e, "agent run loop failed"),
                }
            });
        } else {
            tracing::info!(agent_id = %self.id, "agent is already running, message queued");
        }

        Ok(need_start)
    }
}

/// Get an existing agent or create a new one.
pub async fn get_or_create_agent(
    state: &AppState,
    id: Option<Uuid>,
    identity: Option<&str>,
) -> anyhow::Result<Arc<AgentHandle>> {
    let agent_id = id.unwrap_or_else(Uuid::new_v4);

    // Check if agent already exists
    {
        let agents = state.agents.read().await;
        if let Some(existing) = agents.get(&agent_id) {
            return Ok(Arc::new(AgentHandle {
                id: existing.id,
                identity: existing.identity.clone(),
                agent: existing.agent.clone(),
                events: existing.events.clone(),
            }));
        }
    }

    // Create a new agent
    let (event_tx, event_rx) = mpsc::unbounded_channel();
    let event_log: Arc<tokio::sync::Mutex<Vec<AgentEvent>>> =
        Arc::new(tokio::sync::Mutex::new(Vec::new()));

    // Try to restore memory from the latest snapshot for this agent.
    let restored_memory = match state.storage.get_latest_snapshot(agent_id).await {
        Ok(Some(snapshot)) => {
            tracing::info!(
                agent_id = %agent_id,
                ts = snapshot.ts,
                memory_items = snapshot.memory.items.len(),
                "restored agent memory from latest snapshot"
            );
            Some(snapshot.memory)
        }
        Ok(None) => {
            tracing::info!(agent_id = %agent_id, "no existing snapshot, starting fresh");
            None
        }
        Err(e) => {
            tracing::warn!(agent_id = %agent_id, error = %e, "failed to load snapshot, starting fresh");
            None
        }
    };

    let tools = iceberg_registrations();
    let mut builder = Agent::builder()
        .with_event_tx(event_tx)
        .with_tools(tools)
        .with_system_prompt_identity(identity.unwrap_or("You are a helpful AI assistant."))
        .with_model_pool(state.model_pool.read().await.clone())
        .with_storage(state.storage.clone())
        .with_id(agent_id);

    if let Some(memory) = restored_memory {
        builder = builder.with_memory(memory);
    }

    let agent = builder.build().await?;

    let handle = Arc::new(AgentHandle {
        id: agent.id(),
        identity: identity.unwrap_or("default").to_string(),
        agent: Arc::new(tokio::sync::Mutex::new(agent)),
        events: event_log.clone(),
    });

    // Spawn bridge task: forward agent events to the per-agent replay log
    // **and** the global broadcast bus atomically (see `AgentHandle::events`).
    let broker_tx = state.event_broker.clone();
    let bridge_agent_id = handle.id;
    tokio::spawn(async move {
        bridge_agent_events(bridge_agent_id, event_rx, broker_tx, event_log).await;
    });

    state.agents.write().await.insert(
        handle.id,
        AgentHandle {
            id: handle.id,
            identity: handle.identity.clone(),
            agent: handle.agent.clone(),
            events: handle.events.clone(),
        },
    );

    Ok(handle)
}

/// Delete an agent from the pool and remove all its persisted snapshots.
pub async fn delete_agent(state: &AppState, agent_id: Uuid) -> anyhow::Result<()> {
    state.agents.write().await.remove(&agent_id);
    state
        .storage
        .delete_agent_snapshots(agent_id)
        .await
        .map_err(|e| anyhow::anyhow!("failed to delete snapshots: {}", e))?;
    tracing::info!(agent_id = %agent_id, "agent deleted (pool + snapshots)");
    Ok(())
}

/// List all known agents by cross-referencing storage snapshots with the live pool.
pub async fn list_agents(state: &AppState) -> anyhow::Result<Vec<crate::routes::agents::AgentInfo>> {
    let agent_ids = state
        .storage
        .list_all_agent_ids()
        .await
        .map_err(|e| anyhow::anyhow!("failed to list agent ids: {}", e))?;

    let pool = state.agents.read().await;
    let mut infos = Vec::new();

    for id in agent_ids {
        let (identity, status) = match pool.get(&id) {
            Some(handle) => (handle.identity.clone(), handle.status().await),
            None => ("(offline)".to_string(), "IDLE".to_string()),
        };

        let last_ts = state
            .storage
            .get_latest_snapshot(id)
            .await
            .ok()
            .flatten()
            .map(|s| s.ts);

        infos.push(crate::routes::agents::AgentInfo {
            id: id.to_string(),
            identity,
            status,
            last_active_ts: last_ts,
        });
    }

    // Sort by last_active_ts descending (most recent first)
    infos.sort_by(|a, b| {
        b.last_active_ts
            .unwrap_or(0)
            .cmp(&a.last_active_ts.unwrap_or(0))
    });

    Ok(infos)
}

/// Get the default agent handle created at startup.
pub async fn get_default_agent(state: &AppState) -> anyhow::Result<AgentHandle> {
    let agent_id = state
        .default_agent_id
        .read()
        .await
        .ok_or_else(|| anyhow::anyhow!("no default agent initialized"))?;
    let agents = state.agents.read().await;
    agents
        .get(&agent_id)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("default agent {} not found in pool", agent_id))
}

/// Bridge task: reads from a single agent's mpsc channel and forwards
/// to the global broadcast bus tagged with the agent's UUID.
///
/// Each event is appended to the per-agent `event_log` **and** broadcast
/// while holding the log's lock. A late subscriber can therefore take an
/// atomic snapshot of the log and then attach to the broadcast bus with
/// no gap (missed events) and no duplication — any event not in the
/// snapshot has not yet been broadcast.
async fn bridge_agent_events(
    agent_id: Uuid,
    mut rx: mpsc::UnboundedReceiver<AgentEvent>,
    tx: tokio::sync::broadcast::Sender<(Uuid, AgentEvent)>,
    event_log: Arc<tokio::sync::Mutex<Vec<AgentEvent>>>,
) {
    while let Some(event) = rx.recv().await {
        let mut log = event_log.lock().await;
        log.push(event.clone());
        let event_name = format!("{:?}", event);
        tracing::debug!(agent_id = %agent_id, event = event_name, log_len = log.len(), "bridge: forwarded event");
        let _ = tx.send((agent_id, event));
        drop(log);
    }
}

/// Persist the default agent UUID into the on-disk config so it survives restarts.
pub async fn persist_default_agent_id(state: &AppState, agent_id: Uuid) -> anyhow::Result<()> {
    let mut config = state.config.write().await;
    let already_persisted = config.default_agent_id == Some(agent_id);
    if already_persisted {
        return Ok(());
    }
    config.default_agent_id = Some(agent_id);
    config.save(&state.config_path)?;
    tracing::info!(agent_id = %agent_id, "persisted default agent id to config");
    Ok(())
}
