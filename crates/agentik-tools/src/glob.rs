use agentik_sdk::types::ToolResult;
use async_trait::async_trait;
use agentik_proc::tool;

use agentik_core::tools::{ToolError, ToolFunction};

const MAX_RESULTS: usize = 100;


#[tool(
    name = "glob",
    description = "Finds files whose path matches a glob pattern (e.g. '**/*.rs'). Returns up to 100 matching paths."
)]
pub struct GlobInput {
    #[desc = "Glob pattern, e.g. '**/*.rs', 'src/*.ts', '*/test*'"]
    pub pattern: String,
    #[desc = "Base directory to search in. Defaults to the current directory."]
    pub path: Option<String>,
}

pub struct GlobTool;

#[async_trait]
impl ToolFunction for GlobTool {
    type Input = GlobInput;

    async fn run(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let base = input.path.unwrap_or_else(|| ".".to_string());
        let full_pattern = if input.pattern.starts_with('/') || input.pattern.starts_with('.') {
            input.pattern.clone()
        } else {
            format!("{base}/{}", input.pattern)
        };

        let mut results: Vec<String> = Vec::new();
        match glob::glob(&full_pattern) {
            Ok(paths) => {
                for entry in paths.flatten() {
                    results.push(entry.display().to_string());
                    if results.len() >= MAX_RESULTS {
                        break;
                    }
                }
            }
            Err(e) => {
                return Ok(ToolResult::error(
                    format!("Invalid glob pattern: {e}"),
                ));
            }
        }

        results.sort();
        let truncated = results.len() >= MAX_RESULTS;
        let mut out = results.join("\n");
        if truncated {
            out.push_str(&format!("\n\n(results limited to {MAX_RESULTS})"));
        }
        if out.is_empty() {
            out = "(no matches)".to_string();
        }
        Ok(ToolResult::success(out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_glob_finds_files() {
        let tool = GlobTool;
        let result = tool
            .run(GlobInput {
                pattern: "*.rs".to_string(),
                path: Some("src".to_string()),
            })
            .await
            .unwrap();
        match &result.content {
            agentik_sdk::types::ToolResultContent::Text(t) => {
                assert!(t.contains("lib.rs"));
            }
            other => panic!("expected text, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_glob_no_match() {
        let tool = GlobTool;
        let result = tool
            .run(GlobInput {
                pattern: "*.nonexistent_xyz".to_string(),
                path: Some("src".to_string()),
            })
            .await
            .unwrap();
        match &result.content {
            agentik_sdk::types::ToolResultContent::Text(t) => assert_eq!(t, "(no matches)"),
            other => panic!("expected text, got {other:?}"),
        }
    }
}
