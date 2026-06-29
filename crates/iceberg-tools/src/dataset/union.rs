//! L3 transform tool: concatenate (union all) two datasets.
//!
//! Wraps [`AetherDataset::union`]. Both datasets must have identical column
//! names and types. The `other` dataset is appended to `name`; the result
//! replaces `name` (or writes to `output`).

use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction};
use agentik_sdk::types::ToolResult;
use async_trait::async_trait;
use data_engine::DatasetStore;
use serde::{Deserialize, Serialize};

use crate::common::err;

pub struct DatasetUnionTool {
    pub store: Arc<DatasetStore>,
}

#[derive(Debug, Deserialize, Serialize, agentik_proc::ToolInput)]
#[tool(
    name = "dataset_union",
    description = "Concatenate two registered datasets (UNION ALL). Both must have identical column names and types. The result replaces the first dataset (name) or writes to `output`."
)]
pub struct DatasetUnionInput {
    #[desc = "Name of the first dataset (base)"]
    pub name: String,
    #[desc = "Name of the second dataset to append"]
    pub other: String,
    #[desc = "Output dataset name. Defaults to the first dataset's name (in-place replace)."]
    pub output: Option<String>,
}

#[async_trait]
impl ToolFunction for DatasetUnionTool {
    type Input = DatasetUnionInput;

    fn timeout_seconds(&self) -> u64 { 300 }

    async fn run(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let name = input.name.trim();
        let other = input.other.trim();
        if name.is_empty() || other.is_empty() {
            return Ok(ToolResult::error("'name' and 'other' are required"));
        }
        if name == other {
            return Ok(ToolResult::error("'name' and 'other' must be different datasets"));
        }

        let ds = self.store.get(name).await.map_err(err)?;
        let other_ds = self.store.get(other).await.map_err(err)?;
        let merged = ds.union(&other_ds).map_err(err)?;

        let output_name = input.output.as_deref().map(str::trim).unwrap_or(name);
        let renamed = data_engine::AetherDataset::with_schema(
            output_name,
            merged.schema().clone(),
            merged.batches().to_vec(),
        );
        self.store.put_overwrite(renamed).await;

        let rows_a = ds.row_count();
        let rows_b = other_ds.row_count();
        Ok(ToolResult::success_json(serde_json::json!({
            "dataset": output_name,
            "row_count": rows_a + rows_b,
            "rows_from_first": rows_a,
            "rows_from_second": rows_b,
        })))
    }
}
