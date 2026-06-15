use std::path::Path;

use agentik_sdk::types::ToolResult;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::tools::{ToolError, ToolFunction};

#[derive(Debug, Deserialize, Serialize, agentik_proc::ToolInput)]
#[tool(
    name = "read",
    description = "Reads a UTF-8 text file and returns it with line numbers (cat -n style). Supports offset/limit for partial reads."
)]
pub struct ReadInput {
    #[desc = "Path to the file to read (absolute or relative to cwd)"]
    pub file_path: String,
    #[desc = "Line number to start reading from, 1-indexed. Defaults to 1."]
    pub offset: Option<usize>,
    #[desc = "Maximum number of lines to read. Defaults to the whole file."]
    pub limit: Option<usize>,
}

pub struct ReadTool;

#[async_trait]
impl ToolFunction for ReadTool {
    type Input = ReadInput;

    async fn run(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let path = Path::new(&input.file_path);
        let content = match fs::read_to_string(path).await {
            Ok(c) => c,
            Err(e) => {
                return Ok(ToolResult::error(
                    String::new(),
                    format!("Failed to read {}: {e}", input.file_path),
                ));
            }
        };

        let lines: Vec<&str> = content.lines().collect();
        let total = lines.len();
        let start = input.offset.unwrap_or(1).saturating_sub(1).min(total);
        let end = match input.limit {
            Some(n) => (start + n).min(total),
            None => total,
        };

        let width = end.to_string().len().max(3);
        let mut out = String::new();
        for (i, line) in lines[start..end].iter().enumerate() {
            let n = start + i + 1;
            out.push_str(&format!("{n:>width$}\t{line}\n"));
        }
        if out.is_empty() {
            out.push_str("(empty file or range)");
        }
        Ok(ToolResult::success(String::new(), out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_read_full() {
        let dir = std::env::temp_dir();
        let path = dir.join("agentik_read_test.txt");
        std::fs::write(&path, "a\nb\nc\n").unwrap();
        let tool = ReadTool;
        let result = tool
            .run(ReadInput {
                file_path: path.display().to_string(),
                offset: None,
                limit: None,
            })
            .await
            .unwrap();
        match &result.content {
            agentik_sdk::types::ToolResultContent::Text(t) => {
                assert!(t.contains("1\ta"));
                assert!(t.contains("3\tc"));
            }
            other => panic!("expected text, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_read_offset_limit() {
        let dir = std::env::temp_dir();
        let path = dir.join("agentik_read_range_test.txt");
        std::fs::write(&path, "1\n2\n3\n4\n5\n").unwrap();
        let tool = ReadTool;
        let result = tool
            .run(ReadInput {
                file_path: path.display().to_string(),
                offset: Some(2),
                limit: Some(2),
            })
            .await
            .unwrap();
        match &result.content {
            agentik_sdk::types::ToolResultContent::Text(t) => {
                assert!(t.contains("2\t2"));
                assert!(t.contains("3\t3"));
                assert!(!t.contains("4\t4"));
            }
            other => panic!("expected text, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_read_missing_file() {
        let tool = ReadTool;
        let result = tool
            .run(ReadInput {
                file_path: "/nonexistent/agentik/nope.txt".to_string(),
                offset: None,
                limit: None,
            })
            .await
            .unwrap();
        assert_eq!(result.is_error, Some(true));
    }
}
