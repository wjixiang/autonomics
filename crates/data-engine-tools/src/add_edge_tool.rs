use std::sync::Arc;

use agentik_sdk::types::ToolResult;
use async_trait::async_trait;
use data_engine::runtime::DataEngineClient;

use agentik_core::tools::{ToolError, ToolFunction};
use agentik_proc::tool;

use crate::ExecError;

#[tool(
    name = "add_edge",
    description = "Connect two DAG nodes: data flows from the 'from' node to \
                  the 'to' node under the given port name (e.g. a table name \
                  a SqlNode can reference). When 'port' is omitted, a name is \
                  auto-generated as '{from}__{to}'."
)]
pub struct AddEdgeInput {
    #[desc = "ID of the upstream (source) node"]
    pub from: String,
    #[desc = "ID of the downstream (target) node"]
    pub to: String,
    #[desc = "Optional port (table alias) name for this edge. Auto-generated \
             as '{from}__{to}' when omitted."]
    pub port: Option<String>,
}

pub struct AddEdgeTool {
    client: Arc<DataEngineClient>,
}

impl AddEdgeTool {
    pub fn new(client: Arc<DataEngineClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl ToolFunction for AddEdgeTool {
    type Input = AddEdgeInput;

    async fn run(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let msg = match &input.port {
            Some(p) => format!("edge added: {} -> {} (port: {})", input.from, input.to, p),
            None => format!("edge added: {} -> {}", input.from, input.to),
        };

        self.client
            .add_edge(input.from, input.to, input.port)
            .await
            .map_err(ExecError::from)?;

        Ok(ToolResult::success(msg))
    }
}
