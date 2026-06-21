use std::path::Path;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::fs;

use agentik_core::tools::{ToolError, ToolFunction, ToolResult};

#[derive(Debug, Deserialize, Serialize, agentik_proc::ToolInput)]
#[tool(
    name = "edit",
    description = "Performs an exact string replacement in a file. Fails if old_string is absent or ambiguous (unless replace_all is set)."
)]
pub struct EditInput {
    #[desc = "Path to the file to edit (absolute or relative to cwd)"]
    pub file_path: String,
    #[desc = "The exact text to replace. Must be unique in the file unless replace_all is true."]
    pub old_string: String,
    #[desc = "The text to replace old_string with. Must differ from old_string."]
    pub new_string: String,
    #[desc = "If true, replace every occurrence of old_string. Defaults to false."]
    pub replace_all: Option<bool>,
}

pub struct EditTool;

#[async_trait]
impl ToolFunction for EditTool {
    type Input = EditInput;

    async fn run(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        if input.old_string == input.new_string {
            return Ok(ToolResult::error(
                "old_string must differ from new_string".to_string(),
            ));
        }

        let path = Path::new(&input.file_path);
        let content = match fs::read_to_string(path).await {
            Ok(c) => c,
            Err(e) => {
                return Ok(ToolResult::error(
                    format!("Failed to read {}: {e}", input.file_path),
                ));
            }
        };

        let replace_all = input.replace_all.unwrap_or(false);
        let count = content.matches(&input.old_string).count();

        if count == 0 {
            return Ok(ToolResult::error(
                "old_string not found in file".to_string(),
            ));
        }
        if count > 1 && !replace_all {
            return Ok(ToolResult::error(
                format!(
                    "old_string matches {count} locations; set replace_all=true or make old_string unique"
                ),
            ));
        }

        let new_content = if replace_all {
            content.replace(&input.old_string, &input.new_string)
        } else {
            content.replacen(&input.old_string, &input.new_string, 1)
        };

        if let Err(e) = fs::write(path, &new_content).await {
            return Ok(ToolResult::error(
                format!("Failed to write {}: {e}", input.file_path),
            ));
        }

        let n = if replace_all { count } else { 1 };
        let plural = if n == 1 { "" } else { "s" };
        Ok(ToolResult::success(
            format!("Edited {} ({} replacement{})", input.file_path, n, plural),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_edit_single() {
        let dir = std::env::temp_dir();
        let path = dir.join("agentik_edit_test.txt");
        std::fs::write(&path, "foo bar baz").unwrap();
        let tool = EditTool;
        let result = tool
            .run(EditInput {
                file_path: path.display().to_string(),
                old_string: "bar".to_string(),
                new_string: "qux".to_string(),
                replace_all: None,
            })
            .await
            .unwrap();
        assert!(result.is_error.is_none());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "foo qux baz");
    }

    #[tokio::test]
    async fn test_edit_ambiguous_fails() {
        let dir = std::env::temp_dir();
        let path = dir.join("agentik_edit_ambig_test.txt");
        std::fs::write(&path, "x x x").unwrap();
        let tool = EditTool;
        let result = tool
            .run(EditInput {
                file_path: path.display().to_string(),
                old_string: "x".to_string(),
                new_string: "y".to_string(),
                replace_all: None,
            })
            .await
            .unwrap();
        assert_eq!(result.is_error, Some(true));
    }

    #[tokio::test]
    async fn test_edit_replace_all() {
        let dir = std::env::temp_dir();
        let path = dir.join("agentik_edit_all_test.txt");
        std::fs::write(&path, "x x x").unwrap();
        let tool = EditTool;
        let result = tool
            .run(EditInput {
                file_path: path.display().to_string(),
                old_string: "x".to_string(),
                new_string: "y".to_string(),
                replace_all: Some(true),
            })
            .await
            .unwrap();
        assert!(result.is_error.is_none());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "y y y");
    }

    #[tokio::test]
    async fn test_edit_not_found() {
        let dir = std::env::temp_dir();
        let path = dir.join("agentik_edit_nf_test.txt");
        std::fs::write(&path, "hello").unwrap();
        let tool = EditTool;
        let result = tool
            .run(EditInput {
                file_path: path.display().to_string(),
                old_string: "missing".to_string(),
                new_string: "x".to_string(),
                replace_all: None,
            })
            .await
            .unwrap();
        assert_eq!(result.is_error, Some(true));
    }
}
