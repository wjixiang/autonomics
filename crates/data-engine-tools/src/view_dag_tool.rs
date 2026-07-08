use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction};
use agentik_proc::tool;
use agentik_sdk::types::ToolResult;
use async_trait::async_trait;
use data_engine::runtime::DataEngineClient;

use crate::ExecError;

#[tool(
    name = "view_dag",
    description = "Render the current DAG topology as a Graphviz DOT string. \
                  Useful for visualising the pipeline structure and node \
                  dependencies before or after execution."
)]
pub struct ViewDagInput {}

pub struct ViewDagTool {
    client: Arc<DataEngineClient>,
}

impl ViewDagTool {
    pub fn new(client: Arc<DataEngineClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl ToolFunction for ViewDagTool {
    type Input = ViewDagInput;

    async fn run(&self, _input: Self::Input) -> Result<ToolResult, ToolError> {
        let dot = self.client.view_dag().await.map_err(ExecError::from)?;
        Ok(ToolResult::success(dot))
    }
}
