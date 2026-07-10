use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction, ToolResult};
use agentik_sdk::types::ToolResult as AgentToolResult;
use async_trait::async_trait;

use agentik_proc::tool;

use crate::storage::OpendalFileStorage;

/// Default max bytes returned when no explicit `limit` is given.
/// Prevents OOM on unexpectedly large files while still being generous
/// enough for most code & text files.
const DEFAULT_READ_LIMIT: u64 = 512 * 1024; // 512 KiB

#[tool(
    name = "file_read",
    description = "Read a file's content (UTF-8 text only). \
        Supports partial reads via `offset` and `limit`. \
        When neither is set, reads up to 512 KiB from the beginning. \
        Returns `content`, `bytes_read` (size of returned content), \
        `total_size` (full file size), and `truncated` (true when the \
        returned content is shorter than the full file)."
)]
pub struct FileReadInput {
    #[desc = "Path to the file to read."]
    pub path: String,
    #[desc = "Byte offset to start reading from. Defaults to 0."]
    pub offset: Option<u64>,
    #[desc = "Maximum number of bytes to read. Defaults to 512 KiB (524 288 bytes) if not set."]
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
        let limit = input.limit.unwrap_or(DEFAULT_READ_LIMIT);

        // Stat the file so we can report total_size and truncated.
        let total_size = op
            .stat(&path)
            .await
            .map(|m| m.content_length())
            .unwrap_or(0);

        // Clamp the read range to the actual file length.
        let effective_limit = limit.min(total_size.saturating_sub(offset));
        if effective_limit == 0 {
            return Ok(AgentToolResult::success_json(serde_json::json!({
                "path": input.path,
                "content": "",
                "bytes_read": 0,
                "total_size": total_size,
                "truncated": false,
            })));
        }

        let reader = op
            .reader(&path)
            .await
            .map_err(|e| e.to_string())?;
        let buf = reader
            .read(offset..offset + effective_limit)
            .await
            .map_err(|e| e.to_string())?;

        let content = buf.to_vec();
        let text = String::from_utf8(content.clone()).map_err(|e| e.to_string())?;
        let bytes_read = content.len() as u64;
        let truncated = offset + bytes_read < total_size;

        Ok(AgentToolResult::success_json(serde_json::json!({
            "path": input.path,
            "content": text,
            "bytes_read": bytes_read,
            "total_size": total_size,
            "truncated": truncated,
        })))
    }
}
