use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction, ToolResult};
use agentik_sdk::types::ToolResult as AgentToolResult;
use async_trait::async_trait;

use agentik_proc::tool;

use crate::storage::OpendalFileStorage;

#[tool(
    name = "file_info",
    description = "Get metadata for a file or directory: size, type, last modified time."
)]
pub struct FileInfoInput {
    #[desc = "Path to the file or directory."]
    pub path: String,
}

pub struct FileInfoTool {
    pub(crate) storage: Arc<OpendalFileStorage>,
}

#[async_trait]
impl ToolFunction for FileInfoTool {
    type Input = FileInfoInput;

    async fn run(&self, input: Self::Input) -> Result<AgentToolResult, ToolError> {
        let op = &self.storage.op;
        let path = OpendalFileStorage::normalize_path(&input.path);
        let meta = op.stat(&path).await.map_err(|e| e.to_string())?;

        Ok(AgentToolResult::success_json(serde_json::json!({
            "path": input.path,
            "is_dir": meta.is_dir(),
            "size": meta.content_length(),
            "last_modified": meta.last_modified().map(|t| t.to_string()).unwrap_or_default(),
        })))
    }
}
