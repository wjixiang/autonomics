use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction};
use agentik_proc::tool;
use agentik_sdk::types::ToolResult;
use async_trait::async_trait;
use data_engine::runtime::DataEngineClient;

use crate::ExecError;

#[tool(
    name = "clear_dag",
    description = "Remove all nodes, edges, and runtime state from the DAG. \
                  Use this to start building a new pipeline from scratch."
)]
pub struct ClearDagInput {}

pub struct ClearDagTool {
    client: Arc<DataEngineClient>,
}

impl ClearDagTool {
    pub fn new(client: Arc<DataEngineClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl ToolFunction for ClearDagTool {
    type Input = ClearDagInput;

    async fn run(&self, _input: Self::Input) -> Result<ToolResult, ToolError> {
        self.client.clear_dag().await.map_err(ExecError::from)?;
        Ok(ToolResult::success("DAG cleared"))
    }
}
