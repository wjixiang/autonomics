//! L3 transform tool: generic SQL transform on registered datasets.
//!
//! Wraps [`DatasetStore::sql_to_dataset`]. The SQL can reference any
//! registered dataset by name (they are automatically registered as
//! DataFusion temporary tables before execution) as well as Iceberg tables
//! via `iceberg."ns"."table"`.
//!
//! Use this for filter (WHERE), join, group_by, distinct, aggregate,
//! window functions, drop/rename columns — anything SQL can express.

use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction};
use agentik_sdk::types::ToolResult;
use async_trait::async_trait;
use datalake::DatasetStore;
use serde::{Deserialize, Serialize};

use crate::common::err;

pub struct DatasetSqlTool {
    pub store: Arc<DatasetStore>,
}

#[derive(Debug, Deserialize, Serialize, agentik_proc::ToolInput)]
#[tool(
    name = "dataset_sql",
    description = "Execute a SQL query against registered datasets and/or Iceberg tables, registering the result as a new (or overwritten) dataset. All registered datasets are available as tables in the SQL context. Iceberg tables use iceberg.\"namespace\".\"table\" syntax. Common uses: filter (WHERE), join (JOIN), aggregate (GROUP BY), distinct (SELECT DISTINCT), rename columns."
)]
pub struct DatasetSqlInput {
    #[desc = "Name for the output dataset (registered in the store)"]
    pub name: String,
    #[desc = "SQL query to execute. Can reference registered datasets by name and Iceberg tables via iceberg.\"ns\".\"tbl\"."]
    pub sql: String,
}

#[async_trait]
impl ToolFunction for DatasetSqlTool {
    type Input = DatasetSqlInput;

    fn timeout_seconds(&self) -> u64 { 300 }

    async fn run(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let name = input.name.trim();
        let sql = input.sql.trim();
        if name.is_empty() || sql.is_empty() {
            return Ok(ToolResult::error("'name' and 'sql' are required"));
        }

        let ds = self
            .store
            .sql_to_dataset(name, sql)
            .await
            .map_err(err)?;

        Ok(ToolResult::success_json(serde_json::json!({
            "dataset": ds.name(),
            "row_count": ds.row_count(),
            "column_count": ds.column_count(),
            "columns": ds.column_names().collect::<Vec<_>>(),
        })))
    }
}
