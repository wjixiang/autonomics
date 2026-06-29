//! L2 inspection tool: describe a registered dataset's schema.
//!
//! `dataset_describe` returns the column-level metadata (name, Arrow data
//! type, nullability) plus row/column counts for a dataset already in the
//! store. It does **not** return row data — use `dataset_preview` for rows.

use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction};
use agentik_sdk::types::ToolResult;
use async_trait::async_trait;
use data_engine::DatasetStore;
use serde::{Deserialize, Serialize};

use crate::common::err;

pub struct DatasetDescribeTool {
    pub store: Arc<DatasetStore>,
}

#[derive(Debug, Deserialize, Serialize, agentik_proc::ToolInput)]
#[tool(
    name = "dataset_describe",
    description = "Describe the schema of an in-memory dataset registered in the store: name, row count, column count, and per-column name/type/nullable. Read-only; does not return row data."
)]
pub struct DatasetDescribeInput {
    #[desc = "Name of the registered dataset to describe"]
    pub name: String,
}

#[async_trait]
impl ToolFunction for DatasetDescribeTool {
    type Input = DatasetDescribeInput;

    async fn run(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let name = input.name.trim();
        if name.is_empty() {
            return Ok(ToolResult::error("'name' is required"));
        }

        let ds = self.store.get(name).await.map_err(err)?;
        let mut schema = ds.schema_json();
        // Enrich with provenance for traceability.
        if let serde_json::Value::Object(ref mut map) = schema {
            map.insert(
                "provenance".to_string(),
                serde_json::Value::String(format!("{}", ds.provenance())),
            );
        }
        Ok(ToolResult::success_json(schema))
    }
}
