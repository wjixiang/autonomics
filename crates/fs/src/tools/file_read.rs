use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction, ToolResult};
use agentik_sdk::types::ToolResult as AgentToolResult;
use async_trait::async_trait;

use agentik_proc::tool;

use crate::storage::OpendalFileStorage;

#[tool(
    name = "file_read",
    description = "Read a file's content. Returns the text content and byte size."
)]
pub struct FileReadInput {
    #[desc = "Path to the file to read."]
    pub path: String,
    #[desc = "Byte offset to start reading from. Defaults to 0."]
    pub offset: Option<u64>,
    #[desc = "Maximum number of bytes to read. Reads the entire file if not set."]
    pub limit: Option<u64>,
}

pub struct FileReadTool {
    pub(crate) storage: Arc<OpendalFileStorage>,
}

#[async_trait]
impl ToolFunction for FileReadTool {
    type Input = FileReadInput;

    async fn run(&self, input: Self::Input) -> Result<AgentToolResult, ToolError> {
        let op = &self.storage.op;
        let path = OpendalFileStorage::normalize_path(&input.path);
        let offset = input.offset.unwrap_or(0);

        let buf = if let Some(limit) = input.limit {
            let reader = op
                .reader(&path)
                .await
                .map_err(|e| e.to_string())?;
            reader
                .read(offset..offset + limit)
                .await
                .map_err(|e| e.to_string())?
        } else {
            op.read(&path).await.map_err(|e| e.to_string())?
        };

        let content = buf.to_vec();
        let text = String::from_utf8(content.clone()).map_err(|e| e.to_string())?;
        let size = content.len() as u64;

        Ok(AgentToolResult::success_json(serde_json::json!({
            "path": input.path,
            "content": text,
            "size": size,
        })))
    }
}
