use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction};
use agentik_proc::tool;
use agentik_sdk::types::ToolResult;

use crate::ExecError;
use async_trait::async_trait;
use data_engine::runtime::DataEngineClient;

#[tool(
    name = "get_output",
    description = "Get the output DataFrames of a node after a DAG run. \
                  The node must have been executed (status Success). Returns \
                  a summary with schema and row count for each output DataFrame."
)]
pub struct GetOutputInput {
    /// The node id to query output for.
    pub id: String,
}

pub struct GetOutputTool {
    client: Arc<DataEngineClient>,
}

impl GetOutputTool {
    pub fn new(client: Arc<DataEngineClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl ToolFunction for GetOutputTool {
    type Input = GetOutputInput;

    async fn run(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let Some(dfs) = self
            .client
            .get_output(input.id.clone())
            .await
            .map_err(ExecError::from)?
        else {
            return Ok(ToolResult::error(format!(
                "no output found for node '{}'",
                input.id
            )));
        };

        let mut outputs_info = Vec::with_capacity(dfs.len());
        for (name, df) in dfs.iter() {
            let schema = df.schema();
            let fields: Vec<serde_json::Value> = schema
                .fields()
                .iter()
                .map(|f| serde_json::json!({
                    "name": f.name(),
                    "type": f.data_type().to_string(),
                }))
                .collect();

            let count = df.clone().count().await.unwrap_or(0);
            outputs_info.push(serde_json::json!({
                "name": name,
                "columns": fields.len(),
                "rows": count,
                "fields": fields,
            }));
        }

        let content = serde_json::json!({
            "node": input.id,
            "output_count": outputs_info.len(),
            "outputs": outputs_info,
        });

        Ok(ToolResult::success_json(content))
    }
}
