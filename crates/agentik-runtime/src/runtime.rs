//! Unified runtime portal that owns the skill server and process manager.
//!
//! [`Runtime`] is the single entry point for the host binary. It:
//! 1. Starts an embedded skill registry gRPC server as a background tokio task.
//! 2. Connects a [`SkillRegistryClient`] to that server.
//! 3. Owns a [`ProcessManager`] (which in turn owns the agent registry and model pool).
//! 4. Provides graceful shutdown in the correct order: agents first, then skill server.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use thiserror::Error;
use tokio::net::TcpListener;

use crate::pool::PoolOwner;
use crate::process::{ProcessEvent, ProcessExitStatus, ProcessManager};
use crate::registry::AgentRegistry;

// ── Error ───────────────────────────────────────────────────────

/// Errors produced by [`Runtime`] initialization or operation.
#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("failed to bind skill server: {0}")]
    Bind(#[source] std::io::Error),

    #[error("skill server failed: {0}")]
    SkillServer(String),

    #[error("skill client connection failed: {0}")]
    SkillClient(#[from] agentik_skill_client::SkillClientError),

    #[error("process manager error: {0}")]
    Process(#[from] crate::process::ProcessError),

    #[error("skill store error: {0}")]
    SkillStore(#[from] agentik_skill_server::store::SkillStoreError),

    #[error("skill store required for import/export but no db_path configured")]
    NoSkillStore,
}

// ── Configuration ───────────────────────────────────────────────

/// Declarative configuration for constructing a [`Runtime`].
#[derive(Default, Clone, Debug)]
pub struct RuntimeConfig {
    /// Address for the embedded skill server.
    /// Use `"127.0.0.1:0"` for OS-assigned port (recommended).
    /// Use `None` to skip starting the skill server entirely.
    pub skill_server_addr: Option<SocketAddr>,

    /// Path to the SQLite database backing the skill store.
    /// Required when `skill_server_addr` is `Some`; the server opens this DB.
    pub db_path: Option<PathBuf>,

    /// Directories to scan for skills on startup (optional initial import).
    /// Ignored when `skill_server_addr` is `None`.
    pub skill_dirs: Vec<PathBuf>,

    /// Initial model configuration for the pool.
    /// Can be `None` to defer pool configuration (call
    /// `ProcessManager::configure_pool` later).
    pub model_config: Option<crate::ModelConfig>,
}

impl RuntimeConfig {
    /// Create a config with an embedded skill server on an OS-assigned port.
    pub fn with_embedded_skill_server(skill_dirs: Vec<PathBuf>) -> Self {
        Self {
            skill_server_addr: Some("127.0.0.1:0".parse().unwrap()),
            skill_dirs,
            ..Default::default()
        }
    }

    /// Create a config with no skill server (headless mode).
    pub fn headless() -> Self {
        Self {
            skill_server_addr: None,
            ..Default::default()
        }
    }

    /// Set the SQLite database path used by the embedded skill server.
    pub fn with_db_path(mut self, path: PathBuf) -> Self {
        self.db_path = Some(path);
        self
    }

    /// Set the initial model configuration.
    pub fn with_model_config(mut self, config: crate::ModelConfig) -> Self {
        self.model_config = Some(config);
        self
    }
}

// ── Runtime ────────────────────────────────────────────────────

/// The unified runtime portal for the agent system.
///
/// Owns the embedded skill server task and the agent process manager.
/// Construct via [`Runtime::new`] which handles the full initialization
/// sequence (bind, start skill server, connect client, configure pool).
pub struct Runtime {
    /// The address the skill server is bound to. `None` if no server was started.
    skill_server_addr: Option<SocketAddr>,

    /// JoinHandle for the embedded skill server task.
    skill_server_handle: Option<tokio::task::JoinHandle<()>>,

    /// Connected skill client (for blueprint `activate_skill` support).
    skill_client: Option<Arc<tokio::sync::Mutex<agentik_skill_client::SkillRegistryClient>>>,

    /// The multi-agent process manager.
    process_manager: ProcessManager,
}

/// Maximum retries and delay when connecting to the embedded skill server.
const CONNECT_MAX_RETRIES: usize = 10;
const CONNECT_RETRY_DELAY: Duration = Duration::from_millis(50);

impl Runtime {
    /// Build and start a [`Runtime`] from the given configuration.
    ///
    /// Initialization sequence:
    /// 1. Bind a TCP listener for the skill server (if configured).
    /// 2. Start the skill server as a background tokio task.
    /// 3. Connect a `SkillRegistryClient` to the skill server.
    /// 4. Create the `ProcessManager` and configure the pool (if provided).
    pub async fn new(config: RuntimeConfig) -> Result<Self, RuntimeError> {
        let registry = Arc::new(AgentRegistry::new());
        let pool = Arc::new(PoolOwner::new());

        // ── Step 1-3: Skill server (optional) ──
        let (skill_server_addr, skill_server_handle, skill_client) =
            if let Some(addr) = config.skill_server_addr {
                let listener = TcpListener::bind(addr).await.map_err(RuntimeError::Bind)?;
                let bound_addr = listener.local_addr().map_err(RuntimeError::Bind)?;

                tracing::info!(%bound_addr, "skill server bound");

                let db_path = config
                    .db_path
                    .clone()
                    .ok_or_else(|| RuntimeError::SkillServer("db_path required when starting embedded skill server".into()))?;
                let skill_dirs = config.skill_dirs.clone();
                let handle = tokio::spawn(async move {
                    if let Err(e) =
                        agentik_skill_server::run_with_listener(listener, db_path, skill_dirs).await
                    {
                        tracing::error!(error = %e, "skill server error");
                    }
                });

                // Retry connection until the server is ready to accept.
                let client_addr = format!("http://{bound_addr}");
                let client = connect_with_retry(&client_addr).await?;

                (Some(bound_addr), Some(handle), Some(Arc::new(tokio::sync::Mutex::new(client))))
            } else {
                (None, None, None)
            };

        // ── Step 4: Process manager ──
        let process_manager = ProcessManager::with_registry_and_pool(registry, pool);

        // Configure pool if provided.
        if let Some(ref model_config) = config.model_config {
            process_manager
                .configure_pool(model_config)
                .await
                .map_err(RuntimeError::Process)?;
        }

        Ok(Self {
            skill_server_addr,
            skill_server_handle,
            skill_client,
            process_manager,
        })
    }

    // ── Accessors ─────────────────────────────────────────────

    /// The address the embedded skill server is listening on.
    /// `None` if the skill server is not configured.
    pub fn skill_server_addr(&self) -> Option<SocketAddr> {
        self.skill_server_addr
    }

    /// Access the process manager for lifecycle control (spawn, start, stop, etc.).
    pub fn process_manager(&self) -> &ProcessManager {
        &self.process_manager
    }

    /// Access the agent registry (for registering agent kinds).
    pub fn registry(&self) -> &AgentRegistry {
        self.process_manager.registry()
    }

    /// The connected skill client, if the skill server is running.
    ///
    /// Agent blueprints should receive this via `Arc<Mutex<SkillRegistryClient>>`.
    pub fn skill_client(
        &self,
    ) -> Option<&Arc<tokio::sync::Mutex<agentik_skill_client::SkillRegistryClient>>> {
        self.skill_client.as_ref()
    }

    /// Subscribe to the aggregated event stream for all agents.
    pub fn events(&self) -> tokio::sync::broadcast::Receiver<ProcessEvent> {
        self.process_manager.events()
    }

    /// Configure the model pool. See [`ProcessManager::configure_pool`].
    pub async fn configure_pool(
        &self,
        cfg: &crate::ModelConfig,
    ) -> Result<(), RuntimeError> {
        self.process_manager
            .configure_pool(cfg)
            .await
            .map_err(RuntimeError::Process)?;
        Ok(())
    }

    /// Reconfigure the pool and rebuild all running agents.
    pub async fn reconfigure_pool(&self, cfg: &crate::ModelConfig) -> Result<usize, RuntimeError> {
        self.process_manager
            .reconfigure_pool(cfg)
            .await
            .map_err(RuntimeError::Process)
    }

    // ── Shutdown ────────────────────────────────────────────

    /// Gracefully shut down the runtime.
    ///
    /// Sequence:
    /// 1. Shut down all agents (cancel + await their tasks).
    /// 2. Abort the skill server task.
    ///
    /// Returns the exit statuses of all agents.
    pub async fn shutdown(self) -> Vec<(uuid::Uuid, ProcessExitStatus)> {
        // Step 1: Shut down agents first.
        let agent_results = self.process_manager.shutdown().await;

        // Step 2: Abort the skill server task (if any).
        if let Some(handle) = self.skill_server_handle {
            handle.abort();
            let _ = handle.await;
        }

        agent_results
    }
}

// ── Helpers ────────────────────────────────────────────────────

/// Retry connecting to the skill server with exponential backoff.
async fn connect_with_retry(
    addr: &str,
) -> Result<agentik_skill_client::SkillRegistryClient, RuntimeError> {
    let mut last_err = None;
    for i in 0..CONNECT_MAX_RETRIES {
        match agentik_skill_client::SkillRegistryClient::connect(addr).await {
            Ok(client) => return Ok(client),
            Err(e) => {
                tracing::debug!(attempt = i + 1, error = %e, "skill server not ready, retrying");
                last_err = Some(e);
                tokio::time::sleep(CONNECT_RETRY_DELAY).await;
            }
        }
    }
    Err(RuntimeError::SkillClient(
        last_err.expect("at least one retry should have been attempted"),
    ))
}
