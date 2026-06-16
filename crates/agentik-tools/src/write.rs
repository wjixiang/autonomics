use std::path::Path;

use agentik_sdk::types::ToolResult;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::fs;

use agentik_core::tools::{ToolError, ToolFunction};

#[derive(Debug, Deserialize, Serialize, agentik_proc::ToolInput)]
#[tool(
    name = "write",
    description = "Creates or overwrites a file with the given content. Parent directories are created if missing."
)]
pub struct WriteInput {
    #[desc = "Path to the file to write (absolute or relative to cwd)"]
    pub file_path: String,
    #[desc = "The full content to write to the file"]
    pub content: String,
}

pub struct WriteTool;

#[async_trait]
impl ToolFunction for WriteTool {
    type Input = WriteInput;

    async fn run(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let path = Path::new(&input.file_path);

        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                if let Err(e) = fs::create_dir_all(parent).await {
                    return Ok(ToolResult::error(
                        String::new(),
                        format!("Failed to create parent directories: {e}"),
                    ));
                }
            }
        }

        let existed = path.exists();
        match fs::write(path, &input.content).await {
            Ok(()) => {
                let verb = if existed { "Updated" } else { "Created" };
                Ok(ToolResult::success(
                    String::new(),
                    format!("{verb} {} ({} bytes)", input.file_path, input.content.len()),
                ))
            }
            Err(e) => Ok(ToolResult::error(
                String::new(),
                format!("Failed to write {}: {e}", input.file_path),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_write_creates_and_parents() {
        let dir = std::env::temp_dir().join("agentik_write_test_dir");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("nested/file.txt");
        let tool = WriteTool;
        let result = tool
            .run(WriteInput {
                file_path: path.display().to_string(),
                content: "hello".to_string(),
            })
            .await
            .unwrap();
        assert!(result.is_error.is_none());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello");
    }
}
