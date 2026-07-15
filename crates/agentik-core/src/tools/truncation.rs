//! Unified tool output truncation.
//!
//! Inspired by OpenCode's `ToolOutputStore.bound()` / `Truncate.output()`.
//! Uses a head/tail preservation strategy: when output exceeds limits, the
//! first half of the budget is taken from the head and the second half from
//! the tail, with a truncation notice in between.

/// Default configuration matching OpenCode's `ToolOutputStore` defaults.
pub const DEFAULT_MAX_LINES: usize = 2_000;
/// Default max bytes per tool output (50 KB), matching OpenCode.
pub const DEFAULT_MAX_BYTES: usize = 50 * 1_024;

/// Configurable limits for tool output truncation.
#[derive(Debug, Clone, Copy)]
pub struct TruncationConfig {
    /// Maximum number of lines to retain. Default: 2000.
    pub max_lines: usize,
    /// Maximum number of bytes to retain. Default: 50 KB.
    pub max_bytes: usize,
}

impl Default for TruncationConfig {
    fn default() -> Self {
        Self {
            max_lines: DEFAULT_MAX_LINES,
            max_bytes: DEFAULT_MAX_BYTES,
        }
    }
}

/// Result of truncating a tool output string.
#[derive(Debug, Clone)]
pub struct TruncatedOutput {
    /// The (possibly truncated) content string.
    pub content: String,
    /// Whether truncation was applied.
    pub truncated: bool,
}

/// Truncate tool output using a head/tail preservation strategy.
///
/// When the output exceeds either the line limit or the byte limit:
/// - `head_lines = ceil(max_lines / 2)` lines are taken from the start
/// - `tail_lines = floor(max_lines / 2)` lines are taken from the end
/// - A truncation notice is inserted between head and tail
///
/// If the output fits within both limits, it is returned unchanged.
pub fn truncate_tool_output(content: &str, config: &TruncationConfig) -> TruncatedOutput {
    let lines: Vec<&str> = content.lines().collect();

    // Check byte limit first (more restrictive)
    if content.len() <= config.max_bytes && lines.len() <= config.max_lines {
        return TruncatedOutput {
            content: content.to_string(),
            truncated: false,
        };
    }

    let head_lines = (config.max_lines + 1) / 2;
    let tail_lines = config.max_lines / 2;

    let mut result = String::new();

    if lines.len() <= config.max_lines {
        // Only byte limit exceeded — split bytes
        let byte_head = (config.max_bytes + 1) / 2;
        let byte_tail = config.max_bytes / 2;

        let total = content.len();
        if total > config.max_bytes {
            let head = &content[..byte_head.min(content.len())];
            let tail_offset = total.saturating_sub(byte_tail);
            let tail = &content[tail_offset..];
            let omitted = total - byte_head - byte_tail.min(total - byte_head);

            use std::fmt::Write;
            write!(
                result,
                "{head}\n\n... [output truncated: omitted {omitted} bytes] ...\n\
                 Use 'read' tool with offset/limit to view the full content.\n\n{tail}"
            )
            .unwrap();
        } else {
            result.push_str(content);
        }
    } else {
        // Line limit exceeded (or both)
        let head_end = head_lines.min(lines.len());
        let tail_start = lines.len().saturating_sub(tail_lines);

        let omitted_lines = lines.len() - head_end - tail_lines.min(lines.len() - head_end);

        for line in &lines[..head_end] {
            result.push_str(line);
            result.push('\n');
        }
        result.push_str(&format!(
            "\n... [output truncated: omitted {omitted_lines} lines] ...\n\
             Use 'read' tool with offset/limit to view the full content.\n"
        ));
        for line in &lines[tail_start..] {
            result.push_str(line);
            result.push('\n');
        }

        // Also enforce byte limit on the result
        if result.len() > config.max_bytes {
            let byte_head = (config.max_bytes + 1) / 2;
            let byte_tail = config.max_bytes / 2;
            let total = result.len();
            let head = &result[..byte_head.min(result.len())];
            let tail_offset = total.saturating_sub(byte_tail);
            let tail = &result[tail_offset..];
            let omitted = total - byte_head - byte_tail.min(total - byte_head);

            result = format!(
                "{head}\n\n... [output truncated: omitted {omitted} bytes] ...\n\
                 Use 'read' tool with offset/limit to view the full content.\n\n{tail}"
            );
        }
    }

    TruncatedOutput {
        content: result,
        truncated: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_truncation_short_content() {
        let config = TruncationConfig::default();
        let output = truncate_tool_output("hello world", &config);
        assert!(!output.truncated);
        assert_eq!(output.content, "hello world");
    }

    #[test]
    fn test_no_truncation_within_limits() {
        let config = TruncationConfig {
            max_lines: 100,
            max_bytes: 10_000,
        };
        let content = "line\n".repeat(50);
        let output = truncate_tool_output(&content, &config);
        assert!(!output.truncated);
        assert_eq!(output.content, content);
    }

    #[test]
    fn test_truncation_by_lines() {
        let config = TruncationConfig {
            max_lines: 10,
            max_bytes: 1_000_000, // high byte limit
        };
        let content = (0..20)
            .map(|i| format!("line {}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let output = truncate_tool_output(&content, &config);
        assert!(output.truncated);
        assert!(output.content.contains("line 0"));
        assert!(output.content.contains("line 19"));
        assert!(
            output
                .content
                .contains("[output truncated: omitted 10 lines]")
        );
    }

    #[test]
    fn test_truncation_by_bytes() {
        let config = TruncationConfig {
            max_lines: 1_000_000, // high line limit
            max_bytes: 100,
        };
        let content = "a".repeat(500);
        let output = truncate_tool_output(&content, &config);
        assert!(output.truncated);
        assert!(output.content.contains("[output truncated"));
        assert!(output.content.contains("omitted"));
    }

    #[test]
    fn test_empty_content() {
        let config = TruncationConfig::default();
        let output = truncate_tool_output("", &config);
        assert!(!output.truncated);
        assert_eq!(output.content, "");
    }

    #[test]
    fn test_exact_boundary() {
        let config = TruncationConfig {
            max_lines: 10,
            max_bytes: 1_000_000,
        };
        let content = (0..10)
            .map(|i| format!("line {}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let output = truncate_tool_output(&content, &config);
        // Exactly at limit — should not truncate
        assert!(!output.truncated);
    }

    #[test]
    fn test_head_tail_preservation() {
        let config = TruncationConfig {
            max_lines: 6, // head=3, tail=3
            max_bytes: 1_000_000,
        };
        let content = (0..10)
            .map(|i| format!("line {}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let output = truncate_tool_output(&content, &config);
        assert!(output.truncated);
        // Head should have lines 0-2
        assert!(output.content.starts_with("line 0"));
        // Tail should have lines 7-9
        assert!(output.content.contains("line 7"));
        assert!(output.content.ends_with("line 9\n"));
    }
}
