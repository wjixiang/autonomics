use std::path::Path;

use agentik_sdk::types::ToolResult;
use async_trait::async_trait;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::tools::{ToolError, ToolFunction};

const MAX_LINES: usize = 250;

#[derive(Debug, Deserialize, Serialize, agentik_proc::ToolInput)]
#[tool(
    name = "grep",
    description = "Searches file contents with a regular expression. Respects .gitignore. Returns matching lines, files, or counts."
)]
pub struct GrepInput {
    #[desc = "Regular expression pattern to search for"]
    pub pattern: String,
    #[desc = "File or directory to search in. Defaults to the current directory."]
    pub path: Option<String>,
    #[desc = "Optional glob to filter files by name, e.g. '*.rs'"]
    pub glob: Option<String>,
    #[desc = "Output mode: 'files_with_matches' (default), 'content', or 'count'"]
    pub output_mode: Option<String>,
}

pub struct GrepTool;

#[async_trait]
impl ToolFunction for GrepTool {
    type Input = GrepInput;

    async fn run(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let re = match Regex::new(&input.pattern) {
            Ok(r) => r,
            Err(e) => {
                return Ok(ToolResult::error(
                    String::new(),
                    format!("Invalid regex: {e}"),
                ));
            }
        };
        let glob_filter = match input.glob.as_deref() {
            Some(g) => match glob::Pattern::new(g) {
                Ok(p) => Some(p),
                Err(e) => {
                    return Ok(ToolResult::error(
                        String::new(),
                        format!("Invalid glob: {e}"),
                    ));
                }
            },
            None => None,
        };

        let search_path = Path::new(input.path.as_deref().unwrap_or("."));
        let mode = input.output_mode.as_deref().unwrap_or("files_with_matches");

        // ignore::Walk respects .gitignore, skips hidden by default-ish.
        let walker = ignore::WalkBuilder::new(search_path)
            .hidden(true)
            .git_ignore(true)
            .git_exclude(true)
            .build();

        let mut matched_files: Vec<String> = Vec::new();
        let mut content_out: Vec<String> = Vec::new();
        let mut count_out: Vec<(String, usize)> = Vec::new();
        let mut total = 0usize;

        for entry in walker {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                continue;
            }
            let path = entry.path();
            if let Some(g) = &glob_filter {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !g.matches(name) {
                    continue;
                }
            }
            let Ok(content) = std::fs::read_to_string(path) else {
                continue;
            };
            let pstr = path.display().to_string();
            let mut file_count = 0usize;
            let mut added_file = false;

            for (i, line) in content.lines().enumerate() {
                if re.is_match(line) {
                    total += 1;
                    file_count += 1;
                    if !added_file {
                        matched_files.push(pstr.clone());
                        added_file = true;
                    }
                    if mode == "content" && content_out.len() < MAX_LINES {
                        content_out.push(format!("{}:{}:{}", pstr, i + 1, line.trim_end()));
                    }
                }
            }
            if mode == "count" && file_count > 0 {
                count_out.push((pstr, file_count));
            }
        }

        let out = match mode {
            "content" => {
                let mut s = content_out.join("\n");
                if content_out.len() >= MAX_LINES {
                    s.push_str(&format!("\n\n(results limited to {MAX_LINES} lines)"));
                }
                if s.is_empty() {
                    s = "(no matches)".to_string();
                }
                s
            }
            "count" => {
                let mut lines: Vec<String> =
                    count_out.iter().map(|(f, c)| format!("{f}:{c}")).collect();
                lines.sort();
                let mut s = lines.join("\n");
                if !lines.is_empty() {
                    s.push_str(&format!("\n\ntotal matches: {total}"));
                } else {
                    s = "(no matches)".to_string();
                }
                s
            }
            _ => {
                matched_files.sort();
                if matched_files.is_empty() {
                    "(no matches)".to_string()
                } else {
                    matched_files.join("\n")
                }
            }
        };

        Ok(ToolResult::success(String::new(), out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_grep_files_with_matches() {
        let tool = GrepTool;
        let result = tool
            .run(GrepInput {
                pattern: "struct ReadTool".to_string(),
                path: Some("src/tools/builtins".to_string()),
                glob: Some("*.rs".to_string()),
                output_mode: None,
            })
            .await
            .unwrap();
        match &result.content {
            agentik_sdk::types::ToolResultContent::Text(t) => {
                assert!(t.contains("read.rs"));
            }
            other => panic!("expected text, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_grep_content_mode() {
        let tool = GrepTool;
        let result = tool
            .run(GrepInput {
                pattern: "pub struct ReadTool".to_string(),
                path: Some("src/tools/builtins/read.rs".to_string()),
                glob: None,
                output_mode: Some("content".to_string()),
            })
            .await
            .unwrap();
        match &result.content {
            agentik_sdk::types::ToolResultContent::Text(t) => {
                assert!(t.contains("pub struct ReadTool"));
            }
            other => panic!("expected text, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_grep_no_match() {
        let tool = GrepTool;
        // Assemble from parts so the literal never appears in this source file.
        let needle = format!("zz{}_nope", "absent_marker_q");
        let result = tool
            .run(GrepInput {
                pattern: needle,
                path: Some("src/tools".to_string()),
                glob: None,
                output_mode: None,
            })
            .await
            .unwrap();
        match &result.content {
            agentik_sdk::types::ToolResultContent::Text(t) => assert_eq!(t, "(no matches)"),
            other => panic!("expected text, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_grep_invalid_regex() {
        let tool = GrepTool;
        let result = tool
            .run(GrepInput {
                pattern: "[unclosed".to_string(),
                path: None,
                glob: None,
                output_mode: None,
            })
            .await
            .unwrap();
        assert_eq!(result.is_error, Some(true));
    }
}
