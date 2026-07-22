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
                  All four arguments are required — there is no default-port \
                  fallback. Use `get_node_ports` to discover the correct \
                  output/input port indices before calling. \
                  \
                  WARNING — DO NOT call `add_edge` in the same response turn as \
                  `add_node`. Both endpoints (`from` and `to`) must already exist \
                  in the DAG when this tool runs; if node creation runs in the \
                  same turn it races ahead of the edge and produces a dangling \
                  edge or a failed connection. First create all nodes and wait \
                  for their results, THEN add edges in a separate turn. Multiple \
                  `add_edge` calls within one turn are fine (assuming every \
                  referenced node already exists)."
)]
pub struct AddEdgeInput {
    #[desc = "ID of the upstream (source) node"]
    pub from: String,
    #[desc = "Output port index on the 'from' node (use `get_node_ports` to look it up)"]
    pub from_port: u8,
    #[desc = "ID of the downstream (target) node"]
    pub to: String,
    #[desc = "Input port index on the 'to' node (use `get_node_ports` to look it up)"]
    pub to_port: u8,
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
        let msg = format!(
            "edge added: {}.{} -> {}.{}",
            input.from, input.from_port, input.to, input.to_port
        );

        self.client
            .add_edge_port(input.from, input.from_port, input.to, input.to_port)
            .await
            .map_err(ExecError::from)?;

        Ok(ToolResult::success(msg))
    }
}
