//! Multi-agent process manager.
//!
//! [`ProcessManager`] spawns, monitors, and controls multiple [`Agent`](agentik_core::Agent)
//! instances as independent tokio tasks.  Each agent gets its own event channel and
//! cancellation token; the manager aggregates all events into a single
//! [`broadcast::Receiver<ProcessEvent>`](ProcessEvent) stream and exposes lifecycle
//! commands (start / stop / restart).

pub mod command;
pub mod error;
pub mod event;

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{broadcast, mpsc, watch};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use agentik_core::agent::Agent;
use agentik_core::agent_builder::AgentBuilder;
use agentik_core::lifecycle::AgentLifecycleStatus;

pub use error::ProcessError;
pub use event::{ProcessEvent, ProcessExitStatus};

use command::Command;

// ── Constants ────────────────────────────────────────────────

/// Buffer size for the aggregated broadcast event stream.
const EVENT_BROADCAST_CAPACITY: usize = 1024;

// ── Per-agent registry entry ─────────────────────────────────

struct ManagerEntry {
    /// Builder (cloned at spawn time) for reconstructing the agent on restart.
    builder: AgentBuilder,
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
/// Maintains a registry of [`Agent`](crate::Agent) instances, each running in its own
/// tokio task.  The manager provides lifecycle commands and aggregates all agent events
/// into a single observable stream.
///
/// # Example
///
/// ```ignore
/// let mut manager = ProcessManager::new();
/// let id = manager.spawn(builder).await?;
/// manager.start(&id)?;
/// let mut events = manager.events();
/// while let Ok(ev) = events.recv().await { /* … */ }
/// manager.shutdown().await;
/// ```
pub struct ProcessManager {
    entries: Arc<tokio::sync::RwLock<HashMap<Uuid, ManagerEntry>>>,
    event_tx: broadcast::Sender<ProcessEvent>,
}

impl Default for ProcessManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessManager {
    /// Create a new, empty process manager.
    pub fn new() -> Self {
        let (event_tx, _) = broadcast::channel(EVENT_BROADCAST_CAPACITY);
        Self {
            entries: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            event_tx,
        }
    }

    // ── Spawn ────────────────────────────────────────────────

    /// Register and prepare a new agent process (not started yet).
    ///
    /// The provided `AgentBuilder` is **cloned** internally so that the agent can be
    /// rebuilt on restart.  Returns the agent's unique ID — call [`start`](Self::start)
    /// to begin execution.
    pub async fn spawn(&self, builder: AgentBuilder) -> Result<Uuid, ProcessError> {
        // Build the agent once so we know the ID immediately.
        let mut agent = builder
            .clone()
            .build()
            .await
            .map_err(|e| ProcessError::AgentFailed {
                agent_id: Uuid::nil(),
                source: e,
            })?;

        let agent_id = agent.id();

        // Per-agent channels.
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let (status_tx, status_rx) = watch::channel(AgentLifecycleStatus::IDLE);
        let cancel_token = CancellationToken::new();
        let (agent_event_tx, agent_event_rx) = mpsc::unbounded_channel();

        // Wire the agent's observation channel.
        agent.event_tx = Some(agent_event_tx);

        // Spawn the agent task.
        let task_handle = tokio::spawn(run_agent_task(
            agent_id,
            agent,
            cancel_token.clone(),
            cmd_rx,
            status_tx.clone(),
        ));

        // Spawn the forwarder task.
        let broadcast_tx = self.event_tx.clone();
        let status_rx_for_forwarder = status_tx.subscribe();
        let task_handle = tokio::spawn(forward_agent_events(
            agent_id,
            agent_event_rx,
            broadcast_tx,
            status_rx_for_forwarder,
            task_handle,
        ));

        // Store the entry.
        let mut entries = self.entries.write().await;
        entries.insert(
            agent_id,
            ManagerEntry {
                builder,
                cmd_tx,
                status_rx,
                cancel_token,
                task_handle,
            },
        );

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

    /// Restart an agent: cancel → rebuild from the stored builder → start again.
    ///
    /// This spawns a background task that waits for the old agent to exit, then
    /// rebuilds and re-spawns it.
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

        // Spawn a background restarter that waits for the old task, rebuilds,
        // and re-registers.
        let builder = entry.builder.clone();
        let broadcast_tx = self.event_tx.clone();
        let agent_id = *agent_id;
        // We need write access to the entries map, so clone a new cmd channel pair.
        let (new_cmd_tx, new_cmd_rx) = mpsc::unbounded_channel();
        let (new_status_tx, _) = watch::channel(AgentLifecycleStatus::IDLE);

        // We capture the manager's entries lock implicitly by spawning a task
        // that will later acquire it.
        let entries_lock = self.entries.clone();

        tokio::spawn(async move {
            // Give the old task a moment to notice cancellation.
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;

            // Rebuild the agent.
            let mut agent = match builder.clone().build().await {
                Ok(a) => a,
                Err(e) => {
                    let _ = broadcast_tx.send(ProcessEvent::ProcessExited {
                        agent_id,
                        status: ProcessExitStatus::Error(format!("rebuild failed: {e}")),
                    });
                    return;
                }
            };

            // Wire event channel.
            let (agent_event_tx, agent_event_rx) = mpsc::unbounded_channel();
            agent.event_tx = Some(agent_event_tx);

            let new_cancel = CancellationToken::new();
            let task_handle = tokio::spawn(run_agent_task(
                agent_id,
                agent,
                new_cancel.clone(),
                new_cmd_rx,
                new_status_tx.clone(),
            ));

            // Spawn forwarder for the new agent.
            let status_rx = new_status_tx.subscribe();
            let forward_handle = tokio::spawn(forward_agent_events(
                agent_id,
                agent_event_rx,
                broadcast_tx,
                status_rx,
                task_handle,
            ));

            // Update the registry entry.
            let mut entries = entries_lock.write().await;
            if let Some(existing) = entries.get_mut(&agent_id) {
                existing.cmd_tx = new_cmd_tx;
                existing.status_rx = new_status_tx.subscribe();
                existing.cancel_token = new_cancel;
                existing.builder = builder;
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
