use std::sync::Arc;

use agentik_core::agent::InternalEvent;
use agentik_core::Agent;
use agentik_sdk::model::model_pool::ModelPool;
use agentik_sdk::types::{AgentEvent, ContentBlock};
use datalake::DatasetStore;
use datalake::aether::AetherWorkspace;
use eutils_rs::EutilsClient;
use file_base::OpendalFileStorage;
use opengwas_rs::OpengwasClient;
use tokio_util::sync::CancellationToken;

/// Bridges the sync TUI thread to the async agent runtime.
///
/// Owns the tokio runtime and a sender for [`InternalEvent`].  The agent
/// is spawned once inside `run()` and communicates exclusively through
/// channels — no `Arc<Mutex<Agent>>` needed.
pub struct AgentRuntime {
    /// Sender to drive the agent's event loop.
    internal_tx: tokio::sync::mpsc::UnboundedSender<InternalEvent>,
    event_rx: tokio::sync::mpsc::UnboundedReceiver<AgentEvent>,
    runtime: tokio::runtime::Runtime,
    cancel_token: CancellationToken,
}

impl AgentRuntime {
    /// Build the agent on the tokio runtime, spawn [`Agent::run`], and
    /// return the runtime wrapper.
    pub fn new(model_pool: ModelPool, system_prompt: &str) -> color_eyre::Result<Self> {
        let runtime = tokio::runtime::Runtime::new()?;
        let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
        let cancel_token = CancellationToken::new();

        // Build the agent inside the runtime, clone the internal sender,
        // then spawn run().  block_on returns the sender.
        let internal_tx = runtime.block_on(async {
            let workspace = Arc::new(
                AetherWorkspace::new()
                    .await
                    .expect("failed to initialise Aether workspace"),
            );
            let store = Arc::new(DatasetStore::from_workspace(&workspace));
            let mut tools = aether_tools::iceberg_registrations(workspace);
            tools.extend(aether_tools::dataset_registrations(store));

            let opengwas = Arc::new(OpengwasClient::new(None));
            let file_storage = Arc::new(OpendalFileStorage::new());
            tools.extend(opengwas_rs::opengwas_registrations(opengwas, file_storage));

            let eutils = Arc::new(EutilsClient::from_env());
            tools.extend(eutils_rs::eutils_registrations(eutils));

            let mut agent = Agent::builder()
                .with_model_pool(Arc::new(model_pool))
                .with_event_tx(event_tx)
                .with_system_prompt_identity(system_prompt)
                .with_tools(tools)
                .build()
                .await
                .expect("failed to build agent");

            let tx = agent.internal_event_tx.clone();
            agent.set_cancel_token(cancel_token.clone());

            // Spawn the agent's event-driven main loop.
            // It runs for the lifetime of the AgentRuntime.
            tokio::spawn(async move {
                agent.run().await;
            });

            tx
        });

        Ok(Self {
            internal_tx,
            event_rx,
            runtime,
            cancel_token,
        })
    }

    /// Send a user message to the agent.
    ///
    /// The message is delivered through the internal event channel.
    /// If the agent is idle it will wake up and start processing;
    /// if already running the message is injected into memory and
    /// picked up on the next workflow iteration.
    pub fn send_message(&self, text: String) {
        let _ = self.internal_tx.send(InternalEvent::MessageInject(vec![
            ContentBlock::Text { text },
        ]));
    }

    /// Cancel the agent.
    ///
    /// Fires the cancellation token which interrupts any in-progress
    /// workflow iteration inside `run()`.
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
