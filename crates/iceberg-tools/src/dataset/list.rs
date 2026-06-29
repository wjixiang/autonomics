//! L2 inspection tool: list all registered in-memory datasets.
//!
//! `dataset_list` wraps [`DatasetStore::list`], returning every named dataset
//! currently held in the agent's working memory along with its row/column
//! counts and column schema. It is read-only and takes no parameters.

use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction};
use agentik_sdk::types::ToolResult;
use async_trait::async_trait;
use data_engine::DatasetStore;
use serde::{Deserialize, Serialize};

pub struct DatasetListTool {
    pub store: Arc<DatasetStore>,
}

#[derive(Debug, Deserialize, Serialize, agentik_proc::ToolInput)]
#[tool(
    name = "dataset_list",
    description = "List all in-memory datasets currently registered in the store. Each entry reports the dataset name, row count, column count, and per-column name/type/nullable schema. Read-only; takes no parameters."
)]
pub struct DatasetListInput {}

#[async_trait]
impl ToolFunction for DatasetListTool {
    type Input = DatasetListInput;

    async fn run(&self, _input: Self::Input) -> Result<ToolResult, ToolError> {
        let datasets = self.store.list().await;
        let count = datasets.len();
        Ok(ToolResult::success_json(serde_json::json!({
            "count": count,
            "datasets": datasets,
        })))
    }
}
