//! L3 transform tool: project to a subset of columns.
//!
//! Wraps [`Dataset::select`] — an in-memory Arrow column projection
//! that avoids SQL parsing overhead. Replaces the dataset in the store by
//! default; pass `output` to write to a different name.

use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction};
use agentik_sdk::types::ToolResult;
use async_trait::async_trait;
use data_engine::DatasetStore;
use serde::{Deserialize, Serialize};

use crate::common::err;

pub struct DatasetSelectTool {
    pub store: Arc<DatasetStore>,
}

#[derive(Debug, Deserialize, Serialize, agentik_proc::ToolInput)]
#[tool(
    name = "dataset_select",
    description = "Project a registered dataset to a subset of columns, replacing it (or writing to `output`). Columns not listed are dropped. This is an in-memory Arrow projection and does not re-read from Iceberg."
)]
pub struct DatasetSelectInput {
    #[desc = "Name of the registered dataset to project"]
    pub name: String,
    #[desc = "Columns to keep (in order)"]
    pub columns: Vec<String>,
    #[desc = "Output dataset name. Defaults to the input name (in-place replace)."]
    pub output: Option<String>,
}

#[async_trait]
impl ToolFunction for DatasetSelectTool {
    type Input = DatasetSelectInput;

    fn timeout_seconds(&self) -> u64 { 300 }

    async fn run(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let name = input.name.trim();
        if name.is_empty() || input.columns.is_empty() {
            return Ok(ToolResult::error("'name' and at least one column are required"));
        }
        let columns: Vec<&str> = input.columns.iter().map(|s| s.trim()).collect();

        let ds = self.store.get(name).await.map_err(err)?;
        let projected = ds.select(&columns).map_err(err)?;

        let mut projected = projected;
        let output_name = input.output.as_deref().map(str::trim).unwrap_or(name);
        if output_name != name {
            // Rename so it registers under the desired name.
            let ds_with_name = data_engine::Dataset::with_schema(
                output_name,
                projected.schema().clone(),
                projected.batches().to_vec(),
            );
            projected = ds_with_name;
        }
        self.store.put_overwrite(projected).await;

        Ok(ToolResult::success_json(serde_json::json!({
            "dataset": output_name,
            "row_count": self.store.get(output_name).await.map_err(err)?.row_count(),
            "column_count": self.store.get(output_name).await.map_err(err)?.column_count(),
            "columns": columns,
        })))
    }
}
