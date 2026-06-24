//! L3 transform tool: take the first N rows of a dataset.
//!
//! Wraps [`AetherDataset::limit`] — a zero-copy slice across Arrow
//! partitions. Replaces the dataset in the store by default.

use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction};
use agentik_sdk::types::ToolResult;
use async_trait::async_trait;
use datalake::DatasetStore;
use serde::{Deserialize, Serialize};

use crate::common::err;

pub struct DatasetLimitTool {
    pub store: Arc<DatasetStore>,
}

#[derive(Debug, Deserialize, Serialize, agentik_proc::ToolInput)]
#[tool(
    name = "dataset_limit",
    description = "Take the first N rows of a registered dataset, replacing it (or writing to `output`). A fast zero-copy operation."
)]
pub struct DatasetLimitInput {
    #[desc = "Name of the registered dataset to limit"]
    pub name: String,
    #[desc = "Maximum number of rows to keep"]
    pub n: usize,
    #[desc = "Output dataset name. Defaults to the input name (in-place replace)."]
    pub output: Option<String>,
}

#[async_trait]
impl ToolFunction for DatasetLimitTool {
    type Input = DatasetLimitInput;

    fn timeout_seconds(&self) -> u64 { 300 }

    async fn run(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let name = input.name.trim();
        if name.is_empty() {
            return Ok(ToolResult::error("'name' is required"));
        }

        let ds = self.store.get(name).await.map_err(err)?;
        let limited = ds.limit(input.n);

        let output_name = input.output.as_deref().map(str::trim).unwrap_or(name);
        let renamed = datalake::AetherDataset::with_schema(
            output_name,
            limited.schema().clone(),
            limited.batches().to_vec(),
        );
        self.store.put_overwrite(renamed).await;

        Ok(ToolResult::success_json(serde_json::json!({
            "dataset": output_name,
            "row_count": self.store.get(output_name).await.map_err(err)?.row_count(),
            "limit": input.n,
            "truncated": ds.row_count() > input.n,
        })))
    }
}
