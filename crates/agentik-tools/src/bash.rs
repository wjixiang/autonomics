use std::process::Stdio;
use std::time::Duration;

use agentik_sdk::types::ToolResult;
use async_trait::async_trait;

use tokio::process::Command;
use tokio::time::timeout;

use agentik_core::tools::truncation::{TruncationConfig, truncate_tool_output};
use agentik_core::tools::{ToolError, ToolFunction};
use agentik_proc::tool;

/// Hard ceiling enforced by the framework wrapper. The real per-command
/// timeout comes from `BashInput::timeout` and is enforced inside `run()`,
/// but we raise the framework ceiling so it never pre-empts a legitimate
/// long-running command.
const FRAMEWORK_TIMEOUT_CEILING_SECS: u64 = 600;
/// Bash tool output uses a higher line limit than the default since command
/// output is often line-oriented. The unified truncation module enforces
/// byte limits; here we raise the line ceiling for bash specifically.
const BASH_MAX_LINES: usize = 2_000;
/// Bash tool byte limit — higher than default to avoid over-truncating
/// command output that the agent may need for debugging.
const BASH_MAX_BYTES: usize = 100 * 1_024; // 100 KB

#[tool(
    name = "bash",
    description = "Executes a bash command and returns its stdout, stderr, and exit code."
)]
pub struct BashInput {
    #[desc = "The bash command to execute"]
    pub command: String,

    #[desc = "Timeout in seconds before the command is killed"]
    pub timeout: usize,

    #[desc = "Short human-readable description of what this command does"]
    pub description: Option<String>,
}

pub struct BashTool;

#[async_trait]
impl ToolFunction for BashTool {
    type Input = BashInput;

    fn timeout_seconds(&self) -> u64 {
        FRAMEWORK_TIMEOUT_CEILING_SECS
    }

    async fn run(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let timeout_secs = input.timeout.max(1);

        let mut command = Command::new("bash");
        command
            .arg("-c")
            .arg(&input.command)
            // piped so we can capture both streams; parent cwd/env are inherited
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // if our future is dropped (timeout), kill the whole process tree
            .kill_on_drop(true);

        // tokio::time::timeout drops the output() future on expiry, which
        // drops the Child; kill_on_drop ensures the process is SIGKILL'd.
        let output = match timeout(Duration::from_secs(timeout_secs as u64), command.output()).await
        {
            Ok(Ok(out)) => out,
            Ok(Err(e)) => {
                return Ok(ToolResult::error(format!("Failed to spawn command: {e}")));
            }
            Err(_) => {
                return Ok(ToolResult::error(format!(
                    "Command timed out after {timeout_secs}s and was killed."
                )));
            }
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let exit_code = output.status.code();

        let content = format_output(&stdout, &stderr, exit_code);

        // Non-zero exit (or signal termination) is surfaced as an error result
        // so the model understands the command failed.
        let is_success = matches!(exit_code, Some(0));
        if is_success {
            Ok(ToolResult::success(content))
        } else {
            Ok(ToolResult::error(content))
        }
    }
}

/// Merge stdout/stderr, annotate the exit code, and truncate the tail.
fn format_output(stdout: &str, stderr: &str, exit_code: Option<i32>) -> String {
    let stdout = stdout.trim();
    let stderr = stderr.trim();

    let mut combined = String::new();
    if !stdout.is_empty() {
        combined.push_str(stdout);
    }
    if !stderr.is_empty() {
        if !combined.is_empty() {
            combined.push_str("\n\n--- stderr ---\n");
        } else {
            combined.push_str("--- stderr ---\n");
        }
        combined.push_str(stderr);
    }

    match exit_code {
        Some(0) => {}
        Some(code) => {
            if !combined.is_empty() {
                combined.push('\n');
            }
            combined.push_str(&format!("[exit code: {code}]"));
        }
        None => {
            if !combined.is_empty() {
                combined.push('\n');
            }
            combined.push_str("[terminated by signal]");
        }
    }

    if combined.is_empty() {
        combined.push_str("(no output)");
    }

    let config = TruncationConfig {
        max_lines: BASH_MAX_LINES,
        max_bytes: BASH_MAX_BYTES,
    };
    truncate_tool_output(&combined, &config).content
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_output_success() {
        let out = format_output("hello\n", "", Some(0));
        assert_eq!(out, "hello");
    }

    #[test]
    fn test_format_output_nonzero_exit() {
        let out = format_output("", "boom", Some(2));
        assert!(out.contains("--- stderr ---"));
        assert!(out.contains("boom"));
        assert!(out.contains("[exit code: 2]"));
    }

    #[test]
    fn test_format_output_no_output() {
        let out = format_output("  \n  ", "\n", Some(0));
        assert_eq!(out, "(no output)");
    }

    #[test]
    fn test_format_output_short() {
        let config = TruncationConfig {
            max_lines: 100,
            max_bytes: 1_000_000,
        };
        assert_eq!(truncate_tool_output("abc", &config).content, "abc");
        assert!(!truncate_tool_output("abc", &config).truncated);
    }

    #[test]
    fn test_format_output_long() {
        let config = TruncationConfig {
            max_lines: 10,
            max_bytes: 1_000_000,
        };
        let s: String = (0..20)
            .map(|i| format!("line {}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let out = truncate_tool_output(&s, &config);
        assert!(out.truncated);
        assert!(out.content.contains("[output truncated"));
    }

    #[tokio::test]
    async fn test_bash_runs_command() {
        let tool = BashTool;
        let input = BashInput {
            command: "echo hello".to_string(),
            timeout: 10,
            description: None,
        };
        let result = tool.run(input).await.unwrap();
        match &result.content {
            agentik_sdk::types::ToolResultContent::Text(t) => assert_eq!(t, "hello"),
            other => panic!("expected text, got {other:?}"),
        }
        assert!(result.is_error.is_none());
    }

    #[tokio::test]
    async fn test_bash_nonzero_exit_marked_error() {
        let tool = BashTool;
        let input = BashInput {
            command: "exit 3".to_string(),
            timeout: 10,
            description: None,
        };
        let result = tool.run(input).await.unwrap();
        assert_eq!(result.is_error, Some(true));
    }

    #[tokio::test]
    async fn test_bash_timeout_kills_process() {
        let tool = BashTool;
        let input = BashInput {
            command: "sleep 30".to_string(),
            timeout: 1,
            description: None,
        };
        let result = tool.run(input).await.unwrap();
        assert_eq!(result.is_error, Some(true));
        match &result.content {
            agentik_sdk::types::ToolResultContent::Text(t) => {
                assert!(t.contains("timed out"), "got: {t}");
            }
            other => panic!("expected text, got {other:?}"),
        }
    }
}
