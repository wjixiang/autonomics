//! L2 inspection tool: preview rows of a registered dataset.
//!
//! `dataset_preview` returns the first N rows of an in-memory dataset as a
//! pretty-printed table, plus the row/column counts and a truncation flag.
//! It reads from working memory (data already loaded via `dataset_load_table`
//! or produced by a transformation) and does not touch Iceberg.

use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction};
use agentik_sdk::types::ToolResult;
use async_trait::async_trait;
use data_engine::DatasetStore;
use serde::{Deserialize, Serialize};

use crate::common::err;

pub struct DatasetPreviewTool {
    pub store: Arc<DatasetStore>,
}

#[derive(Debug, Deserialize, Serialize, agentik_proc::ToolInput)]
#[tool(
    name = "dataset_preview",
    description = "Preview the first N rows of an in-memory dataset already registered in the store, returned as a pretty-printed table. Defaults to 20 rows. Read-only; the dataset must have been loaded or produced previously (e.g. via dataset_load_table). Use dataset_peek_table to preview directly from an Iceberg table without loading it."
)]
pub struct DatasetPreviewInput {
    #[desc = "Name of the registered dataset to preview"]
    pub name: String,
    #[desc = "Maximum number of rows to return. Defaults to 20."]
    pub limit: Option<usize>,
}

#[async_trait]
impl ToolFunction for DatasetPreviewTool {
    type Input = DatasetPreviewInput;

    async fn run(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let name = input.name.trim();
        if name.is_empty() {
            return Ok(ToolResult::error("'name' is required"));
        }
        let limit = input.limit.unwrap_or(20).max(1);

        let ds = self.store.get(name).await.map_err(err)?;
        let total = ds.row_count();
        let rows = ds.pretty_head(limit).map_err(err)?;

        Ok(ToolResult::success_json(serde_json::json!({
            "dataset": name,
            "row_count": total,
            "column_count": ds.column_count(),
            "limit": limit,
            "truncated": total > limit,
            "rows": rows,
        })))
    }
}
