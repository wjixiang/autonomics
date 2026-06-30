use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction, ToolResult};
use agentik_sdk::types::ToolResult as AgentToolResult;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::storage::OpendalFileStorage;

#[derive(Debug, Deserialize, Serialize, agentik_proc::ToolInput)]
#[tool(
    name = "file_write",
    description = "Create or overwrite a file with the given content."
)]
pub struct FileWriteInput {
    #[desc = "Path to the file to write."]
    pub path: String,
    #[desc = "The content to write to the file."]
    pub content: String,
}

pub struct FileWriteTool {
    pub(crate) storage: Arc<OpendalFileStorage>,
}

#[async_trait]
impl ToolFunction for FileWriteTool {
    type Input = FileWriteInput;

    async fn run(&self, input: Self::Input) -> Result<AgentToolResult, ToolError> {
        let op = &self.storage.op;
        let size = input.content.len() as u64;

        op.write(&input.path, input.content.into_bytes())
            .await
            .map_err(|e| e.to_string())?;

        Ok(AgentToolResult::success_json(serde_json::json!({
            "path": input.path,
            "size": size,
        })))
    }
}
