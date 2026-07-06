use std::sync::Arc;

use agentik_core::Agent;
use agentik_core::agent::InternalEvent;
use agentik_sdk::model::model_pool::ModelPool;
use agentik_sdk::types::{AgentEvent, ContentBlock};
use data_engine::data_engine::DataEngine;
use data_engine::runtime::spawn_with_engine;
use fs::OpendalFileStorage;
use tokio_util::sync::CancellationToken;

pub struct AgentRuntime {
    internal_tx: tokio::sync::mpsc::UnboundedSender<InternalEvent>,
    event_rx: tokio::sync::mpsc::UnboundedReceiver<AgentEvent>,
    _engine_handle: tokio::task::JoinHandle<()>,
    cancel_token: CancellationToken,
}

impl AgentRuntime {
    pub fn new(
        runtime: &tokio::runtime::Runtime,
        model_pool: ModelPool,
        system_prompt: &str,
    ) -> anyhow::Result<Self> {
        let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
        let cancel_token = CancellationToken::new();

        let file_storage = Arc::new(OpendalFileStorage::new("/mnt/disk3/test"));

        let (internal_tx, engine_handle) = runtime.block_on(async {
            // Build and spawn DataEngine actor
            let engine = DataEngine::builder()
                .register_opendal_fs(file_storage.clone())
                .unwrap()
                .build();
            let (data_engine_client, engine_handle) = spawn_with_engine(engine);

            let tool_list =
                crate::tools::default_tool_set(file_storage, Arc::new(data_engine_client))?;

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

            Ok::<_, anyhow::Error>((tx, engine_handle))
        })?;

        Ok(Self {
            internal_tx,
            event_rx,
            _engine_handle: engine_handle,
            cancel_token,
        })
    }

    pub fn send_message(&self, text: String) {
        let _ = self
            .internal_tx
            .send(InternalEvent::MessageInject(vec![ContentBlock::Text {
                text,
            }]));
    }

    pub fn cancel(&self) {
        self.cancel_token.cancel();
    }

    pub fn poll_event(&mut self) -> Option<AgentEvent> {
        self.event_rx.try_recv().ok()
    }
}
