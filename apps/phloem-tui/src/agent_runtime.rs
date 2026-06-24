use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use agentik_core::Agent;
use agentik_sdk::model::model_pool::ModelPool;
use agentik_sdk::types::{AgentEvent, ContentBlock};
use datalake::aether::AetherWorkspace;
use datalake::DatasetStore;
use tokio::sync::Mutex;

/// Bridges the sync TUI thread to the async agent runtime.
///
/// Owns the tokio runtime, the agent (behind an `Arc<Mutex>`), and the
/// `mpsc::UnboundedReceiver<AgentEvent>` that the TUI polls each frame.
pub struct AgentRuntime {
    agent: Arc<Mutex<Agent>>,
    event_rx: tokio::sync::mpsc::UnboundedReceiver<AgentEvent>,
    runtime: tokio::runtime::Runtime,
    running: Arc<AtomicBool>,
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
        })
    }

    /// Inject a user message and spawn the agent loop on the tokio runtime.
    ///
    /// If the agent is already running, the message is queued in memory
    /// and will be picked up on the next iteration.
    pub fn send_message(&self, text: String) {
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
                let _ = agent.start().await;
                running.store(false, Ordering::Relaxed);
            }
            // If already running, the injected message will be picked up
            // on the next loop iteration automatically.
        });
    }

    /// Non-blocking poll: drain all pending events from the channel.
    ///
    /// Returns `Some(event)` for each event, call in a loop until `None`.
    pub fn poll_event(&mut self) -> Option<AgentEvent> {
        self.event_rx.try_recv().ok()
    }
}
