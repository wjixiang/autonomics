use std::sync::Arc;

use agentik_core::Agent;
use agentik_core::agent::InternalEvent;
use agentik_core::error::AgentError;
use agentik_sdk::model::model_pool::ModelPool;
use agentik_sdk::types::{AgentEvent, ContentBlock};
use data_engine::data_engine::DataEngine;
use data_engine::runtime::spawn_with_engine;
use datalake::Datalake;
use fs::OpendalFileStorage;
use thiserror::Error;
use tokio_util::sync::CancellationToken;

/// Errors that can occur while building or driving an [`AgentRuntime`].
#[derive(Debug, Error)]
pub enum RuntimeError {
    /// The agent could not be assembled (e.g. model pool misconfiguration,
    /// missing required tools, internal initialization failure).
    #[error("failed to build agent: {0}")]
    AgentBuild(#[from] AgentError),

    #[error("{0}")]
    Engine(#[from] data_engine::data_engine::error::Error),
}

pub type Result<T> = std::result::Result<T, RuntimeError>;

pub struct AgentRuntime {
    internal_tx: tokio::sync::mpsc::UnboundedSender<InternalEvent>,
    event_rx: tokio::sync::mpsc::UnboundedReceiver<AgentEvent>,
    _engine_handle: tokio::task::JoinHandle<()>,
    /// Handle for the spawned agent task, so we can abort it on forced shutdown.
    agent_handle: tokio::task::JoinHandle<()>,
    cancel_token: CancellationToken,
}

impl AgentRuntime {
    pub fn new(runtime: &tokio::runtime::Runtime, model_pool: ModelPool) -> Result<Self> {
        let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
        let cancel_token = CancellationToken::new();

        let file_storage = Arc::new(OpendalFileStorage::new("/mnt/disk3/test"));

        let (internal_tx, engine_handle, agent_handle) = runtime.block_on(async {
            // Build and spawn DataEngine actor
            let engine = DataEngine::builder()
                .register_opendal_fs(file_storage.clone())?
                .register_iceberg().await?
                .build();
            let (data_engine_client, engine_handle) = spawn_with_engine(engine);

            let datalake = Arc::new(Datalake::new());

            let tool_list = crate::tools::default_tool_set(
                file_storage,
                datalake,
                Arc::new(data_engine_client),
            );

            let system_prompt = "\
## Core Competencies

You are a biomedical research assistant with expertise in genomics, GWAS analysis, \
and literature mining. You have direct access to specialized tools — use them \
proactively rather than answering from memory alone.

### Literature & Evidence
- Search PubMed, fetch full article records, retrieve summaries, and find related articles.
- Always verify claims against primary literature when possible.

### Genomics & GWAS (OpenGWAS API)
- Search GWAS datasets by trait or keyword, inspect metadata, download summary statistics.
- Perform variant lookups (by rsID or chr:pos), extract associations, run PheWAS, \
  LD clumping, and compute LD matrices.
- Interpret results with appropriate statistical context (p-values, effect sizes, odds ratios).

### Data Pipeline (DAG Engine)
- Build and execute data processing pipelines: add data sources, apply SQL transforms, \
  connect nodes into a DAG, run the pipeline, and retrieve output.
- Use this when a task requires multi-step data processing or transformation.
- **DAG construction order**: always add all nodes first, then connect them with add_edge, then run_dag.
- **SQL table naming**: in add_sql_node, upstream data is registered as tables named \
  `port_N` where N is the input port index (0-based). For single-input nodes the table is \
  `port_0`. Never use the upstream node's id — always use `port_N`. \
  Example: a filter node receiving one input → write `SELECT * FROM port_0 WHERE x > 1`. \
  A two-input join node → write `SELECT * FROM port_0 JOIN port_1 ON port_0.id = port_1.id`.

### General
- Read, write, and manage files on the local filesystem.
- Break complex research questions into sequential tool calls; explain your reasoning.

## Guidelines
- Cite PMID(s) when referencing literature.
- Report quantitative results with appropriate precision and confidence intervals when available.
- If a tool call fails, diagnose the error and retry with corrected parameters before asking the user.";

            let mut agent = Agent::builder()
                .with_model_pool(Arc::new(model_pool))
                .with_agent_event_tx(event_tx)
                .with_system_prompt_identity(
                    "You are a biomedical research assistant specializing in genomics, \
                     GWAS analysis, and literature mining.",
                )
                .with_system_prompt_section(system_prompt)
                .with_tools(tool_list)
                .with_cancel_token(cancel_token.clone())
                .build()
                .await?;

            let tx = agent.internal_event_tx();

            let agent_handle = tokio::spawn(async move {
                agent.run().await;
            });

            Ok::<_, RuntimeError>((tx, engine_handle, agent_handle))
        })?;

        Ok(Self {
            internal_tx,
            event_rx,
            _engine_handle: engine_handle,
            agent_handle,
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

    pub fn cancel(&mut self) {
        self.cancel_token.cancel();
        // Create a fresh token for the next session.  CancellationToken is
        // one-shot, so without this every subsequent session would see
        // `is_cancelled() == true` and abort immediately.
        let new_token = CancellationToken::new();
        let _ = self
            .internal_tx
            .send(InternalEvent::ResetCancelToken(new_token.clone()));
        self.cancel_token = new_token;
    }

    /// Force-stop the agent: abort the background task and drop the event
    /// channel so the TUI can exit immediately.  Used when the user
    /// double-presses Ctrl+C (cooperative cancel didn't take effect).
    pub fn shutdown(&mut self) {
        let _ = self.internal_tx.send(InternalEvent::Shutdown);
        self.agent_handle.abort();
    }

    pub fn poll_event(&mut self) -> Option<AgentEvent> {
        self.event_rx.try_recv().ok()
    }
}
