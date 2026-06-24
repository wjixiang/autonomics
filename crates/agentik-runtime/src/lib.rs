use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use agentik_core::Agent;
use agentik_sdk::model::model_pool::ModelPool;
use agentik_sdk::types::{AgentEvent, ContentBlock};
use datalake::DatasetStore;
use datalake::aether::AetherWorkspace;
use eutils_rs::EutilsClient;
use file_base::OpendalFileStorage;
use opengwas_rs::OpengwasClient;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

/// Bridges the sync TUI thread to the async agent runtime.
///
/// Owns the tokio runtime, the agent (behind an `Arc<Mutex>`), and the
/// `mpsc::UnboundedReceiver<AgentEvent>` that the TUI polls each frame.
pub struct AgentRuntime {
    agent: Arc<Mutex<Agent>>,
    event_rx: tokio::sync::mpsc::UnboundedReceiver<AgentEvent>,
    runtime: tokio::runtime::Runtime,
    running: Arc<AtomicBool>,
    /// Cancellation token for the *current* agent run.  Replaced with a fresh
    /// token each time a new message is sent so that a prior cancel does not
    /// prevent the next run from starting.
    cancel_token: CancellationToken,
}

impl AgentRuntime {
    /// Build the agent on the tokio runtime and return the runtime wrapper.
    ///
    /// The Aether workspace and its tools are initialised inside the runtime
    /// so that the caller (sync TUI thread) never touches async code.
    pub fn new(model_pool: ModelPool, system_prompt: &str) -> color_eyre::Result<Self> {
        let runtime = tokio::runtime::Runtime::new()?;
        let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();

        let agent = runtime.block_on(async {
            let workspace = Arc::new(
                AetherWorkspace::new()
                    .await
                    .expect("failed to initialise Aether workspace"),
            );
            // The dataset store shares the Aether workspace's DataFusion
            // session so Iceberg tables are visible to dataset tools.
            let store = Arc::new(DatasetStore::from_workspace(&workspace));
            let mut tools = aether_tools::iceberg_registrations(workspace);
            tools.extend(aether_tools::dataset_registrations(store));

            let opengwas = Arc::new(OpengwasClient::new(None::<String>));
            let file_storage = Arc::new(OpendalFileStorage::new());
            tools.extend(opengwas_rs::opengwas_registrations(opengwas, file_storage));

            let eutils = Arc::new(EutilsClient::from_env());
            tools.extend(eutils_rs::eutils_registrations(eutils));

            Agent::builder()
                .with_model_pool(Arc::new(model_pool))
                .with_event_tx(event_tx)
                .with_system_prompt_identity(system_prompt)
                .with_tools(tools)
                .build()
                .await
        })?;

        Ok(Self {
            agent: Arc::new(Mutex::new(agent)),
            event_rx,
            runtime,
            running: Arc::new(AtomicBool::new(false)),
            cancel_token: CancellationToken::new(),
        })
    }

    /// Inject a user message and spawn the agent loop on the tokio runtime.
    ///
    /// If the agent is already running, the message is queued in memory
    /// and will be picked up on the next iteration.
    pub fn send_message(&mut self, text: String) {
        // Create a fresh cancellation token for each run so that a prior
        // cancel does not prevent the next run from starting.
        let cancel_token = CancellationToken::new();
        self.cancel_token = cancel_token.clone();

        let agent = Arc::clone(&self.agent);
        let running = Arc::clone(&self.running);
        let rt = self.runtime.handle();

        rt.spawn(async move {
            let mut agent = agent.lock().await;
            agent
                .inject_message(vec![ContentBlock::Text { text }])
                .expect("failed to inject message");

            if !running.load(Ordering::Relaxed) {
                running.store(true, Ordering::Relaxed);
                // Inject a fresh token before starting so `select!` races
                // against *this* token rather than a stale (already-cancelled)
                // one from a previous run.
                agent.set_cancel_token(cancel_token);
                let _ = agent.start().await;
                running.store(false, Ordering::Relaxed);
            }
            // If already running, the injected message will be picked up
            // on the next loop iteration automatically.
        });
    }

    /// Cancel the currently running agent loop.
    ///
    /// Safe to call even when the agent is idle — it is a no-op in that case.
    pub fn cancel(&self) {
        self.cancel_token.cancel();
    }

    /// Non-blocking poll: drain all pending events from the channel.
    ///
    /// Returns `Some(event)` for each event, call in a loop until `None`.
    pub fn poll_event(&mut self) -> Option<AgentEvent> {
        self.event_rx.try_recv().ok()
    }
}
