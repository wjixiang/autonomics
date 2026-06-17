//! Multi-agent process manager.
//!
//! [`ProcessManager`] spawns, monitors, and controls multiple [`Agent`](agentik_core::Agent)
//! instances as independent tokio tasks.  Each agent gets its own event channel and
//! cancellation token; the manager aggregates all events into a single
//! [`broadcast::Receiver<ProcessEvent>`](ProcessEvent) stream and exposes lifecycle
//! commands (start / stop / restart).
//!
//! ## Architecture
//!
//! The manager owns a shared [`ModelPool`](agentik_sdk::model::model_pool::ModelPool)
//! singleton (via [`PoolOwner`](crate::pool::PoolOwner)) and an
//! [`AgentRegistry`](crate::registry::AgentRegistry).  The frontend spawns agents
//! by registered kind name + declarative options; the runtime builds the concrete
//! `Agent` internally using the kind's context/tools factories and the shared pool.
//! This ensures the frontend never depends on `agentik-core` or `agentik-sdk` types
//! directly — `agentik-runtime` is the sole interface boundary.

pub mod command;
pub mod error;
pub mod event;

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{broadcast, mpsc, watch};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use agentik_core::agent::Agent;
use agentik_core::lifecycle::AgentLifecycleStatus;

use crate::pool::{PoolBuildError, PoolOwner};
use crate::registry::{AgentBlueprint, AgentBlueprintError, AgentRegistry, AgentSpawnOpts};

pub use error::ProcessError;
pub use event::{ProcessEvent, ProcessExitStatus};

use command::Command;

// ── Constants ────────────────────────────────────────────────

/// Buffer size for the aggregated broadcast event stream.
const EVENT_BROADCAST_CAPACITY: usize = 1024;

// ── Per-agent registry entry ─────────────────────────────────

struct ManagerEntry {
    /// The registered agent kind — used to rebuild agents on restart.
    kind: Arc<AgentBlueprint>,
    /// Frontend-supplied spawn options (pure data, no core types).
    spawn_opts: AgentSpawnOpts,
    /// Command sender — the manager sends lifecycle commands here.
    cmd_tx: mpsc::UnboundedSender<Command>,
    /// Receiver that mirrors the agent's current lifecycle status.
    status_rx: watch::Receiver<AgentLifecycleStatus>,
    /// Token for cooperatively cancelling this agent's task.
    cancel_token: CancellationToken,
    /// Handle to the forwarder task. Each forwarder awaits its own agent task,
    /// so awaiting this handle waits for *both* the agent task and the forwarder
    /// to finish — used by [`shutdown`](ProcessManager::shutdown).
    task_handle: tokio::task::JoinHandle<ProcessExitStatus>,
}

// ── ProcessManager ────────────────────────────────────────────

/// Multi-agent process manager.
///
/// Maintains a registry of [`Agent`](agentik_core::Agent) instances, each running in its own
/// tokio task.  The manager provides lifecycle commands and aggregates all agent events
/// into a single observable stream.
///
/// Agents are spawned by registered **kind name** via [`spawn_by_kind`](Self::spawn_by_kind);
/// the model pool is configured declaratively via [`configure_pool`](Self::configure_pool).
/// The frontend never touches `AgentBuilder`, `ModelPool`, or any `agentik-core` type.
///
/// # Example
///
/// ```ignore
/// let mut manager = ProcessManager::new();
///
/// // Configure the shared model pool.
/// manager.configure_pool(&model_config).await?;
///
/// // Spawn an agent by registered kind.
/// let id = manager.spawn_by_kind("coder", AgentSpawnOpts::default()).await?;
/// manager.start(&id)?;
///
/// // Observe events.
/// let mut events = manager.events();
/// while let Ok(ev) = events.recv().await { /* … */ }
///
/// manager.shutdown().await;
/// ```
#[derive(Clone)]
pub struct ProcessManager {
    entries: Arc<tokio::sync::RwLock<HashMap<Uuid, ManagerEntry>>>,
    event_tx: broadcast::Sender<ProcessEvent>,
    /// Shared model-pool singleton.
    pool: Arc<PoolOwner>,
    /// Agent kind registry.
    registry: Arc<AgentRegistry>,
}

impl Default for ProcessManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessManager {
    /// Create a new, empty process manager with an empty pool and registry.
    pub fn new() -> Self {
        let (event_tx, _) = broadcast::channel(EVENT_BROADCAST_CAPACITY);
        Self {
            entries: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            event_tx,
            pool: Arc::new(PoolOwner::new()),
            registry: Arc::new(AgentRegistry::new()),
        }
    }

    /// Create a manager with a pre-populated registry and pool.
    pub fn with_registry_and_pool(
        registry: Arc<AgentRegistry>,
        pool: Arc<PoolOwner>,
    ) -> Self {
        let (event_tx, _) = broadcast::channel(EVENT_BROADCAST_CAPACITY);
        Self {
            entries: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            event_tx,
            pool,
            registry,
        }
    }

    /// Access the agent kind registry (e.g. for the host to register kinds).
    pub fn registry(&self) -> &AgentRegistry {
        &self.registry
    }

    // ── Pool configuration ───────────────────────────────────

    /// Configure the shared model pool from declarative config.
    ///
    /// Must succeed before any [`spawn_by_kind`](Self::spawn_by_kind) call.
    pub async fn configure_pool(
        &self,
        cfg: &crate::model_config::ModelConfig,
    ) -> Result<(), ProcessError> {
        self.pool
            .configure(cfg)
            .await
            .map_err(ProcessError::PoolBuild)?;
        Ok(())
    }

    /// Reconfigure the shared model pool and rebuild **all** running agents
    /// onto the new pool.
    ///
    /// **Known limitation:** rebuilding creates fresh agents (new `Memory`),
    /// so conversation history is lost.  This matches dendrite's
    /// `rebuild_all_agents` behaviour.  Preserving memory across a pool swap
    /// requires core support and is left as a future improvement.
    ///
    /// Returns the number of agents that were rebuilt.
    pub async fn reconfigure_pool(
        &self,
        cfg: &crate::model_config::ModelConfig,
    ) -> Result<usize, ProcessError> {
        self.pool
            .reconfigure(cfg)
            .await
            .map_err(ProcessError::PoolBuild)?;

        // Collect entries to rebuild (id, kind, opts).
        let to_rebuild: Vec<(Uuid, Arc<AgentBlueprint>, AgentSpawnOpts)> = {
            let entries = self.entries.read().await;
            entries
                .iter()
                .map(|(id, e)| (*id, e.kind.clone(), e.spawn_opts.clone()))
                .collect()
        };

        let mut rebuilt = 0usize;
        for (id, kind, opts) in to_rebuild {
            if let Err(e) = self.rebuild_agent(&id, kind, opts).await {
                let _ = self.event_tx.send(ProcessEvent::ProcessExited {
                    agent_id: id,
                    status: ProcessExitStatus::Error(format!("rebuild failed: {e}")),
                });
            } else {
                rebuilt += 1;
            }
        }
        Ok(rebuilt)
    }

    /// Return the model names in the current pool (empty if unconfigured).
    pub async fn pool_model_names(&self) -> Vec<String> {
        self.pool.model_names().await
    }

    // ── Spawn by kind ────────────────────────────────────────

    /// Spawn a new agent by registered kind name.
    ///
    /// The runtime looks up the [`AgentBlueprint`] for `kind`, calls
    /// [`build_agent`](AgentBlueprint::build_agent) with the shared model pool,
    /// and applies any frontend-supplied overrides from `opts`.
    ///
    /// Returns the agent's unique ID — call [`start`](Self::start) to begin execution.
    pub async fn spawn_by_kind(
        &self,
        kind: &str,
        opts: AgentSpawnOpts,
    ) -> Result<Uuid, ProcessError> {
        let kind_entry = self
            .registry
            .get(kind)
            .ok_or_else(|| ProcessError::Kind(AgentBlueprintError::NotFound(kind.to_string())))?;

        let pool = self
            .pool
            .current()
            .await
            .ok_or(ProcessError::PoolNotConfigured)?;

        // Build the agent via AgentBlueprint (skill tree + tool provider + context).
        let mut agent = kind_entry.build_agent(pool).await.map_err(|e| ProcessError::Kind(e))?;

        // Apply prompt overrides — frontend wins.
        if let Some(ident) = &opts.system_prompt_identity {
            agent.set_system_prompt_identity(ident.clone());
        }
        if let Some(section) = &opts.system_prompt_section {
            agent.set_system_prompt_section(section.clone());
        }

        let agent_id = agent.id();

        // Optional initial message.
        if let Some(msg) = opts.initial_message.clone() {
            let _ = agent.inject_message(msg);
        }

        // Register the entry and spawn tasks.
        self.insert_agent(agent_id, agent, kind_entry, opts).await;

        Ok(agent_id)
    }

    // ── Lifecycle commands ────────────────────────────────────

    /// Start a managed agent (IDLE → RUNNING).
    pub fn start(&self, agent_id: &Uuid) -> Result<(), ProcessError> {
        let entries = self
            .entries
            .try_read()
            .map_err(|_| ProcessError::Shutdown)?;
        let entry = entries
            .get(agent_id)
            .ok_or(ProcessError::NotFound(*agent_id))?;
        entry
            .cmd_tx
            .send(Command::Start)
            .map_err(|_| ProcessError::ChannelClosed(*agent_id))
    }

    /// Stop a running agent by cancelling its task.
    pub fn stop(&self, agent_id: &Uuid) -> Result<(), ProcessError> {
        let entries = self
            .entries
            .try_read()
            .map_err(|_| ProcessError::Shutdown)?;
        let entry = entries
            .get(agent_id)
            .ok_or(ProcessError::NotFound(*agent_id))?;
        entry.cancel_token.cancel();
        entry
            .cmd_tx
            .send(Command::Stop)
            .map_err(|_| ProcessError::ChannelClosed(*agent_id))
    }

    /// Restart an agent: cancel → rebuild from the stored kind → start again.
    ///
    /// Calls [`AgentBlueprint::build_agent`] fresh, so skill tree + tools are
    /// preserved across restart.
    ///
    /// This spawns a background task that waits for the old agent to exit, then
    /// rebuilds and re-registers.
    pub fn restart(&self, agent_id: &Uuid) -> Result<(), ProcessError> {
        let entries = self
            .entries
            .try_read()
            .map_err(|_| ProcessError::Shutdown)?;
        let entry = entries
            .get(agent_id)
            .ok_or(ProcessError::NotFound(*agent_id))?;

        // Cancel the current agent and send the restart command so the
        // agent task exits cleanly.
        entry.cancel_token.cancel();
        entry
            .cmd_tx
            .send(Command::Restart)
            .map_err(|_| ProcessError::ChannelClosed(*agent_id))?;

        // Capture what we need for the background rebuild.
        let kind = entry.kind.clone();
        let opts = entry.spawn_opts.clone();
        let broadcast_tx = self.event_tx.clone();
        let agent_id = *agent_id;
        let entries_lock = self.entries.clone();
        let pool = self.pool.clone();

        tokio::spawn(async move {
            // Give the old task a moment to notice cancellation.
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;

            // Rebuild the agent using the kind + current pool.
            let Some(pool_arc) = pool.current().await else {
                let _ = broadcast_tx.send(ProcessEvent::ProcessExited {
                    agent_id,
                    status: ProcessExitStatus::Error("pool not configured".to_string()),
                });
                return;
            };

            let mut agent = match kind.build_agent(pool_arc).await {
                Ok(a) => a,
                Err(e) => {
                    let _ = broadcast_tx.send(ProcessEvent::ProcessExited {
                        agent_id,
                        status: ProcessExitStatus::Error(format!("rebuild failed: {e}")),
                    });
                    return;
                }
            };

            // Apply prompt overrides.
            if let Some(ident) = &opts.system_prompt_identity {
                agent.set_system_prompt_identity(ident.clone());
            }
            if let Some(section) = &opts.system_prompt_section {
                agent.set_system_prompt_section(section.clone());
            }

            // Wire event channel and spawn tasks.
            let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
            let (status_tx, _) = watch::channel(AgentLifecycleStatus::IDLE);
            let cancel_token = CancellationToken::new();
            let (agent_event_tx, agent_event_rx) = mpsc::unbounded_channel();

            agent.event_tx = Some(agent_event_tx);

            let forward_task = tokio::spawn(run_agent_task(
                agent_id,
                agent,
                cancel_token.clone(),
                cmd_rx,
                status_tx.clone(),
            ));

            let status_rx = status_tx.subscribe();
            let forward_handle = tokio::spawn(forward_agent_events(
                agent_id,
                agent_event_rx,
                broadcast_tx,
                status_rx,
                forward_task,
            ));

            // Update the registry entry.
            let mut entries = entries_lock.write().await;
            if let Some(existing) = entries.get_mut(&agent_id) {
                existing.cmd_tx = cmd_tx;
                existing.status_rx = status_tx.subscribe();
                existing.cancel_token = cancel_token;
                existing.task_handle = forward_handle;
            }
        });

        Ok(())
    }

    /// Inject a user message into an agent's memory.
    pub fn inject_message(
        &self,
        agent_id: &Uuid,
        content: Vec<agentik_sdk::types::messages::ContentBlock>,
    ) -> Result<(), ProcessError> {
        let entries = self
            .entries
            .try_read()
            .map_err(|_| ProcessError::Shutdown)?;
        let entry = entries
            .get(agent_id)
            .ok_or(ProcessError::NotFound(*agent_id))?;
        entry
            .cmd_tx
            .send(Command::InjectMessage(content))
            .map_err(|_| ProcessError::ChannelClosed(*agent_id))
    }

    // ── Observation ──────────────────────────────────────────

    /// Get the current lifecycle status of a specific agent.
    pub fn status(&self, agent_id: &Uuid) -> Result<AgentLifecycleStatus, ProcessError> {
        let entries = self
            .entries
            .try_read()
            .map_err(|_| ProcessError::Shutdown)?;
        let entry = entries
            .get(agent_id)
            .ok_or(ProcessError::NotFound(*agent_id))?;
        Ok(*entry.status_rx.borrow())
    }

    /// Subscribe to a specific agent's status changes.
    pub fn status_watch(
        &self,
        agent_id: &Uuid,
    ) -> Result<watch::Receiver<AgentLifecycleStatus>, ProcessError> {
        let entries = self
            .entries
            .try_read()
            .map_err(|_| ProcessError::Shutdown)?;
        let entry = entries
            .get(agent_id)
            .ok_or(ProcessError::NotFound(*agent_id))?;
        Ok(entry.status_rx.clone())
    }

    /// Subscribe to the aggregated event stream for **all** agents.
    pub fn events(&self) -> broadcast::Receiver<ProcessEvent> {
        self.event_tx.subscribe()
    }

    /// List all managed agent IDs.
    pub async fn agent_ids(&self) -> Vec<Uuid> {
        self.entries.read().await.keys().copied().collect()
    }

    /// Return the number of managed agents.
    pub async fn len(&self) -> usize {
        self.entries.read().await.len()
    }

    /// Check if the manager has no agents.
    pub async fn is_empty(&self) -> bool {
        self.entries.read().await.is_empty()
    }

    // ── Shutdown ────────────────────────────────────────────

    /// Shut down all agents and consume the manager.
    ///
    /// Cancels every running agent, **awaits its forwarder task** (which in turn
    /// awaits the agent task), and returns each agent's real exit status.
    ///
    /// Note: cancellation is cooperative. If an agent is currently blocked inside
    /// a non-cancel-aware `start()`, it will only unwind at that call's next await
    /// point; shutdown waits for as long as that takes.
    pub async fn shutdown(self) -> Vec<(Uuid, ProcessExitStatus)> {
        let mut guard = self.entries.write().await;

        // Cancel every agent's task so they begin unwinding.
        for entry in guard.values() {
            entry.cancel_token.cancel();
        }

        // Drain the forwarder handles. Each forwarder owns the agent task's
        // JoinHandle, so awaiting a forwarder waits for both the agent task and
        // the forwarder to finish.
        let handles: Vec<(Uuid, tokio::task::JoinHandle<ProcessExitStatus>)> = guard
            .drain()
            .map(|(id, entry)| (id, entry.task_handle))
            .collect();
        drop(guard);

        // Wait for every agent + forwarder to finish and collect real statuses.
        let mut results = Vec::with_capacity(handles.len());
        for (id, handle) in handles {
            let status = match handle.await {
                Ok(status) => status,
                Err(e) if e.is_panic() => ProcessExitStatus::Panicked(
                    e.into_panic()
                        .downcast_ref::<&str>()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| "unknown panic".to_string()),
                ),
                Err(_) => ProcessExitStatus::Cancelled,
            };
            results.push((id, status));
        }
        results
    }

    // ── Internal helpers ─────────────────────────────────────

    /// Insert a pre-built agent into the entry map and spawn its tasks.
    async fn insert_agent(
        &self,
        agent_id: Uuid,
        mut agent: Agent,
        kind: Arc<AgentBlueprint>,
        opts: AgentSpawnOpts,
    ) {
        // Per-agent channels.
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let (status_tx, status_rx) = watch::channel(AgentLifecycleStatus::IDLE);
        let cancel_token = CancellationToken::new();
        let (agent_event_tx, agent_event_rx) = mpsc::unbounded_channel();

        // Wire the agent's observation channel.
        agent.event_tx = Some(agent_event_tx);

        // Spawn the agent task.
        let agent_task = tokio::spawn(run_agent_task(
            agent_id,
            agent,
            cancel_token.clone(),
            cmd_rx,
            status_tx.clone(),
        ));

        // Spawn the forwarder task.
        let broadcast_tx = self.event_tx.clone();
        let status_rx_for_forwarder = status_tx.subscribe();
        let forward_handle = tokio::spawn(forward_agent_events(
            agent_id,
            agent_event_rx,
            broadcast_tx,
            status_rx_for_forwarder,
            agent_task,
        ));

        // Store the entry.
        let mut entries = self.entries.write().await;
        entries.insert(
            agent_id,
            ManagerEntry {
                kind,
                spawn_opts: opts,
                cmd_tx,
                status_rx,
                cancel_token,
                task_handle: forward_handle,
            },
        );
    }

    /// Rebuild a single agent in-place (cancel → rebuild → re-register).
    /// Used by `reconfigure_pool`.
    async fn rebuild_agent(
        &self,
        agent_id: &Uuid,
        kind: Arc<AgentBlueprint>,
        opts: AgentSpawnOpts,
    ) -> Result<(), ProcessError> {
        // Cancel the existing agent.
        {
            let entries = self.entries.read().await;
            let entry = entries.get(agent_id).ok_or(ProcessError::NotFound(*agent_id))?;
            entry.cancel_token.cancel();
            let _ = entry.cmd_tx.send(Command::Restart);
        }

        // Wait briefly for the old task to exit.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Rebuild via AgentBlueprint.
        let pool = self
            .pool
            .current()
            .await
            .ok_or(ProcessError::PoolNotConfigured)?;

        let mut agent = kind.build_agent(pool).await.map_err(ProcessError::Kind)?;

        // Apply prompt overrides.
        if let Some(ident) = &opts.system_prompt_identity {
            agent.set_system_prompt_identity(ident.clone());
        }
        if let Some(section) = &opts.system_prompt_section {
            agent.set_system_prompt_section(section.clone());
        }

        // New channels and tasks.
        let (new_cmd_tx, new_cmd_rx) = mpsc::unbounded_channel();
        let (new_status_tx, _) = watch::channel(AgentLifecycleStatus::IDLE);
        let new_cancel = CancellationToken::new();
        let (agent_event_tx, agent_event_rx) = mpsc::unbounded_channel();
        agent.event_tx = Some(agent_event_tx);

        let agent_task = tokio::spawn(run_agent_task(
            *agent_id,
            agent,
            new_cancel.clone(),
            new_cmd_rx,
            new_status_tx.clone(),
        ));

        let broadcast_tx = self.event_tx.clone();
        let status_rx = new_status_tx.subscribe();
        let forward_handle = tokio::spawn(forward_agent_events(
            *agent_id,
            agent_event_rx,
            broadcast_tx,
            status_rx,
            agent_task,
        ));

        // Update the entry.
        let mut entries = self.entries.write().await;
        if let Some(existing) = entries.get_mut(agent_id) {
            existing.kind = kind;
            existing.spawn_opts = opts;
            existing.cmd_tx = new_cmd_tx;
            existing.status_rx = new_status_tx.subscribe();
            existing.cancel_token = new_cancel;
            existing.task_handle = forward_handle;
        }

        Ok(())
    }
}

// ── Agent task ───────────────────────────────────────────────

/// The per-agent task: receives commands and drives the agent lifecycle.
async fn run_agent_task(
    _agent_id: Uuid,
    mut agent: Agent,
    cancel_token: CancellationToken,
    mut cmd_rx: mpsc::UnboundedReceiver<Command>,
    status_tx: watch::Sender<AgentLifecycleStatus>,
) -> ProcessExitStatus {
    loop {
        tokio::select! {
            // ── Incoming command ──
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(Command::Start) => {
                        let _ = status_tx.send(AgentLifecycleStatus::RUNNING);
                        let result = agent.start().await;
                        match result {
                            Ok(()) => {
                                let _ = status_tx.send(AgentLifecycleStatus::IDLE);
                                return ProcessExitStatus::Completed;
                            }
                            Err(e) => {
                                let _ = status_tx.send(AgentLifecycleStatus::ABORTED);
                                return ProcessExitStatus::Error(e.to_string());
                            }
                        }
                    }
                    Some(Command::Stop) => {
                        cancel_token.cancel();
                        return ProcessExitStatus::Stopped;
                    }
                    Some(Command::Restart) => {
                        cancel_token.cancel();
                        return ProcessExitStatus::Cancelled;
                    }
                    Some(Command::InjectMessage(content)) => {
                        let _ = agent.inject_message(content);
                        // Continue the loop to process more commands.
                    }
                    None => {
                        // Command channel closed — manager dropped this entry.
                        return ProcessExitStatus::Cancelled;
                    }
                }
            }
            // ── Cancellation ──
            _ = cancel_token.cancelled() => {
                return ProcessExitStatus::Cancelled;
            }
        }
    }
}

// ── Forwarder task ────────────────────────────────────────────

/// Forwards agent events and lifecycle changes to the manager's broadcast stream,
/// and detects agent-task exit.
async fn forward_agent_events(
    agent_id: Uuid,
    mut agent_event_rx: mpsc::UnboundedReceiver<agentik_sdk::types::AgentUiEvent>,
    broadcast_tx: broadcast::Sender<ProcessEvent>,
    mut status_rx: watch::Receiver<AgentLifecycleStatus>,
    mut task_handle: tokio::task::JoinHandle<ProcessExitStatus>,
) -> ProcessExitStatus {
    let mut last_status = *status_rx.borrow();

    loop {
        tokio::select! {
            // Forward agent-level events.
            event = agent_event_rx.recv() => {
                match event {
                    Some(ev) => {
                        let _ = broadcast_tx.send(ProcessEvent::Agent {
                            agent_id,
                            event: ev,
                        });
                    }
                    None => {
                        // Agent event channel closed — agent task likely exited.
                        return ProcessExitStatus::Cancelled;
                    }
                }
            }
            // Detect lifecycle state changes.
            _ = status_rx.changed() => {
                let new_status = *status_rx.borrow();
                if new_status != last_status {
                    last_status = new_status;
                    let _ = broadcast_tx.send(ProcessEvent::StateChanged {
                        agent_id,
                        new_status,
                    });
                }
            }
            // Detect agent task exit.
            result = &mut task_handle => {
                let exit_status = match result {
                    Ok(status) => status,
                    Err(e) if e.is_panic() => {
                        ProcessExitStatus::Panicked(
                            e.into_panic()
                                .downcast_ref::<&str>()
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| "unknown panic".to_string()),
                        )
                    }
                    Err(_) => ProcessExitStatus::Cancelled,
                };
                let _ = broadcast_tx.send(ProcessEvent::ProcessExited {
                    agent_id,
                    status: exit_status.clone(),
                });
                return exit_status;
            }
        }
    }
}
