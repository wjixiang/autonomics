//! L2 lifecycle tool: drop a registered dataset from the store.
//!
//! `dataset_drop` removes a named dataset from working memory. This frees the
//! in-memory data and unregisters the name so it can be reused. It does not
//! affect the underlying Iceberg source.

use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction};
use agentik_sdk::types::ToolResult;
use async_trait::async_trait;
use datalake::DatasetStore;
use serde::{Deserialize, Serialize};

use crate::common::err;

pub struct DatasetDropTool {
    pub store: Arc<DatasetStore>,
}

#[derive(Debug, Deserialize, Serialize, agentik_proc::ToolInput)]
#[tool(
    name = "dataset_drop",
    description = "Remove a named in-memory dataset from the store, freeing its working memory and releasing the name for reuse. Does not affect the underlying Iceberg table. Errors if no dataset with that name exists."
)]
pub struct DatasetDropInput {
    #[desc = "Name of the registered dataset to drop"]
    pub name: String,
}

#[async_trait]
impl ToolFunction for DatasetDropTool {
    type Input = DatasetDropInput;

    async fn run(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let name = input.name.trim();
        if name.is_empty() {
            return Ok(ToolResult::error("'name' is required"));
        }

        // UFCS: an inherent method named `drop` on `Arc<DatasetStore>` would
        // otherwise resolve to `Arc`'s destructor — call it explicitly.
        DatasetStore::drop(&self.store, name).await.map_err(err)?;

        Ok(ToolResult::success_json(serde_json::json!({
            "dropped": name,
        })))
    }
}
