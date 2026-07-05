use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction, ToolResult};
use agentik_sdk::types::ToolResult as AgentToolResult;
use async_trait::async_trait;

use crate::storage::OpendalFileStorage;
use agentik_proc::tool;

#[tool(
    name = "file_delete",
    description = "Delete a file or directory."
)]
pub struct FileDeleteInput {
    #[desc = "Path to the file or directory to delete."]
    pub path: String,
}

pub struct FileDeleteTool {
    pub(crate) storage: Arc<OpendalFileStorage>,
}

#[async_trait]
impl ToolFunction for FileDeleteTool {
    type Input = FileDeleteInput;

    async fn run(&self, input: Self::Input) -> Result<AgentToolResult, ToolError> {
        let op = &self.storage.op;

        op.delete(&input.path)
            .await
            .map_err(|e| e.to_string())?;

        Ok(AgentToolResult::success_json(serde_json::json!({
            "path": input.path,
            "deleted": true,
        })))
    }
}
