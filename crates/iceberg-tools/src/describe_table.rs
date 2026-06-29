//! Aether tool: describe a table's DataFusion-level schema (column names and Arrow types).

use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction};
use agentik_sdk::types::ToolResult;
use async_trait::async_trait;
use data_engine::data_session::DataSession;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, agentik_proc::ToolInput)]
#[tool(
    name = "iceberg_describe_table",
    description = "Describe an Iceberg table's schema as seen by DataFusion. Returns column names, Arrow data types, and nullability. Use this to understand a table's structure before querying it."
)]
pub struct IcebergDescribeTableInput {
    #[desc = "Namespace (schema) containing the table, e.g. 'analytics'. This is a single top-level namespace segment as registered in the DataFusion catalog."]
    pub namespace: String,
    #[desc = "Table name to describe"]
    pub table: String,
}

pub struct IcebergDescribeTableTool {
    pub workspace: Arc<DataSession>,
}

#[async_trait]
impl ToolFunction for IcebergDescribeTableTool {
    type Input = IcebergDescribeTableInput;

    async fn run(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let namespace = input.namespace.trim();
        let table = input.table.trim();

        if namespace.is_empty() || table.is_empty() {
            return Ok(ToolResult::error(
                "both 'namespace' and 'table' are required",
            ));
        }

        let schema = self.workspace
            .describe_table(namespace, table)
            .await
            .map_err(|e| ToolError::ExecutionFailed { source: e.into() })?;

        Ok(ToolResult::success_json(schema))
    }
}
