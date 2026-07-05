use std::sync::Arc;

use agentik_sdk::types::ToolResult;
use async_trait::async_trait;
use data_engine::runtime::DataEngineClient;

use agentik_core::tools::{ToolError, ToolFunction};

use agentik_proc::tool;

#[tool(
    name = "run_dag",
    description = "Execute the current DAG pipeline. All nodes are validated \
                  and run according to their dependency order. Returns a report \
                  with the status of every node."
)]
pub struct RunDagInput {}

pub struct RunDagTool {
    client: Arc<DataEngineClient>,
}

impl RunDagTool {
    pub fn new(client: Arc<DataEngineClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl ToolFunction for RunDagTool {
    type Input = RunDagInput;

    async fn run(&self, _input: Self::Input) -> Result<ToolResult, ToolError> {
        let report = self
            .client
            .run_dag()
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let nodes: serde_json::Map<String, serde_json::Value> = report
            .statuses
            .iter()
            .map(|(id, status)| (id.clone(), serde_json::json!(format!("{:?}", status))))
            .collect();

        let content = serde_json::json!({
            "ok": report.ok,
            "nodes": serde_json::Value::Object(nodes),
            "errors": report.errors.len(),
        });

        if report.ok {
            Ok(ToolResult::success_json(content))
        } else {
            Ok(ToolResult::error(serde_json::to_string(&content).unwrap_or_default()))
        }
    }
}
