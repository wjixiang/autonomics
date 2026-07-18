use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction};
use agentik_proc::tool;
use agentik_sdk::types::ToolResult;
use async_trait::async_trait;
use data_engine::runtime::DataEngineClient;

use crate::ExecError;

#[tool(
    name = "list_node_factories",
    description = "List all registered node kinds with their JSON Schemas. \
                  Each entry reports the node `kind` (used when creating nodes) \
                  and the `schema` describing the parameters the node accepts. \
                  Use this to discover what node types are available before \
                  calling add_node."
)]
pub struct ListNodeFactoriesInput {}

pub struct ListNodeFactoriesTool {
    client: Arc<DataEngineClient>,
}

impl ListNodeFactoriesTool {
    pub fn new(client: Arc<DataEngineClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl ToolFunction for ListNodeFactoriesTool {
    type Input = ListNodeFactoriesInput;

    async fn run(&self, _input: Self::Input) -> Result<ToolResult, ToolError> {
        let nodes = self
            .client
            .list_node_factories()
            .await
            .map_err(ExecError::from)?;

        let content = serde_json::to_value(&nodes)
            .map_err(|e| ToolError::ExecutionFailed {
                source: Box::new(e),
            })?;

        Ok(ToolResult::success_json(content))
    }
}
