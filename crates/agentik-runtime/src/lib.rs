//! Sync-to-async bridge for running an agentik agent from a sync context.
//!
//! [`AgentRuntime`] owns a sender for [`InternalEvent`]. The agent is
//! spawned once inside [`new`] and communicates exclusively through
//! channels — no `Arc<Mutex<Agent>>` needed.
//!
//! The caller is responsible for keeping the tokio runtime alive for
//! the lifetime of this struct.

pub mod tools;

use std::sync::Arc;

use agentik_core::Agent;
use agentik_core::agent::InternalEvent;
use agentik_sdk::model::model_pool::ModelPool;
use agentik_sdk::types::{AgentEvent, ContentBlock};
use file_base::OpendalFileStorage;
use tokio_util::sync::CancellationToken;

/// Bridges the sync TUI thread to the async agent runtime.
///
/// Owns the channel endpoints and a cancellation token. The agent is
/// spawned once inside [`new`] and communicates exclusively through
/// channels — no `Arc<Mutex<Agent>>` needed.
pub struct AgentRuntime {
    /// Sender to drive the agent's event loop.
    internal_tx: tokio::sync::mpsc::UnboundedSender<InternalEvent>,
    event_rx: tokio::sync::mpsc::UnboundedReceiver<AgentEvent>,
    cancel_token: CancellationToken,
}

impl AgentRuntime {
    /// Build the agent on the provided tokio runtime, spawn [`Agent::run`],
    /// and return the bridge.
    ///
    /// The caller **must** keep `runtime` alive for the entire lifetime of
    /// this `AgentRuntime` — the agent's event loop task runs on that runtime.
    pub fn new(
        runtime: &tokio::runtime::Runtime,
        model_pool: ModelPool,
        system_prompt: &str,
    ) -> anyhow::Result<Self> {
        let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
        let cancel_token = CancellationToken::new();

        let file_storage = Arc::new(OpendalFileStorage::new());

        let internal_tx = runtime.block_on(async {
            let tool_list = tools::default_tool_set(file_storage).await?;

            let mut agent = Agent::builder()
                .with_model_pool(Arc::new(model_pool))
                .with_agent_event_tx(event_tx)
                .with_system_prompt_identity(system_prompt)
                .with_tools(tool_list)
                .with_cancel_token(cancel_token.clone())
                .build()
                .await?;

            let tx = agent.internal_event_tx();

            tokio::spawn(async move {
                agent.run().await;
            });

            Ok::<_, anyhow::Error>(tx)
        })?;

        Ok(Self {
            internal_tx,
            event_rx,
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
        let _ = self
            .internal_tx
            .send(InternalEvent::MessageInject(vec![ContentBlock::Text {
                text,
            }]));
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
