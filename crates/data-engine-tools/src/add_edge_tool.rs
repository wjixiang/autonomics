use std::sync::Arc;

use agentik_sdk::types::ToolResult;
use async_trait::async_trait;
use data_engine::runtime::DataEngineClient;

use agentik_core::tools::{ToolError, ToolFunction};
use agentik_proc::tool;

use crate::ExecError;

#[tool(
    name = "add_edge",
    description = "Connect two DAG nodes port-to-port: data flows from the \
                  'from' node's output port to the 'to' node's input port. \
                  Omit from_port/to_port for single-port nodes (default ports)."
)]
pub struct AddEdgeInput {
    #[desc = "ID of the upstream (source) node"]
    pub from: String,
    #[desc = "Optional output port name on the 'from' node. Omit for single-output nodes."]
    pub from_port: Option<String>,
    #[desc = "ID of the downstream (target) node"]
    pub to: String,
    #[desc = "Optional input port name on the 'to' node. Omit for single-input nodes."]
    pub to_port: Option<String>,
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
        let msg = match (&input.from_port, &input.to_port) {
            (Some(fp), Some(tp)) => {
                format!("edge added: {}.{} -> {}.{}", input.from, fp, input.to, tp)
            }
            _ => format!("edge added: {} -> {}", input.from, input.to),
        };

        let res = match (input.from_port, input.to_port) {
            (Some(fp), Some(tp)) => self.client.add_edge_port(input.from, fp, input.to, tp).await,
            _ => self.client.add_edge(input.from, input.to).await,
        };
        res.map_err(ExecError::from)?;

        Ok(ToolResult::success(msg))
    }
}
