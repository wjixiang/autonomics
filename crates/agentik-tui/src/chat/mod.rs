//! Reusable chat-message types and rendering helpers for the
//! conversation history shown in the chat panel.
//!
//! The [`ChatMessage`] enum covers everything a single chat turn
//! can produce: a user prompt, a streaming assistant reply, a
//! thinking block, a tool call/result pair, a done marker, an
//! error, or a blank divider. [`ChatMessage::to_lines`] flattens a
//! single message into the `Vec<Line>` the panel renderer wraps
//! in a `Paragraph`.
//!
//! The [`input`] sub-module owns the bottom input / status row:
//! the text buffer, the activation flag, the spinner frames, and
//! the renderer that draws `> {text}` / spinner / idle hint
//! depending on the host's runtime state.

pub mod events;
pub mod input;
pub mod mouse;
pub mod paste;
pub mod renderer;
pub mod state;
pub mod theme;

use ratatui::style::Style;
use ratatui::text::{Line, Span};
use serde_json::Value;

pub use mouse::{ChatMouseOutcome, MouseEventKind, MouseButton};
pub use paste::{PasteEntry, PASTE_SUMMARY_LEN_THRESHOLD, PASTE_SUMMARY_LINE_THRESHOLD};
pub use state::ChatPanelState;
pub use theme::{ChatPanelTheme, DefaultChatPanelTheme};
const MAX_THINKING_LINES: usize = 10;
const MAX_TOOL_RESULT_LINES: usize = 6;
const MAX_ARRAY_ITEMS: usize = 8;
/// Maximum number of keys from a tool call/result JSON object that
/// the chat panel will render before folding the rest into a single
/// "… and N more keys" line.
const MAX_OBJECT_KEYS: usize = 10;
/// Per-line character cap for any single field value rendered in a
/// tool call/result.
const MAX_TOOL_FIELD_CHARS: usize = 200;
/// Maximum number of *logical* lines from a user message that the
/// chat panel will display verbatim.
const MAX_USER_MESSAGE_LINES: usize = 10;

#[derive(Debug, Clone)]
pub enum ChatMessage {
    User {
        text: String,
    },
    Assistant {
        text: String,
        /// `true` while the LLM is still streaming tokens into this
        /// message.
        streaming: bool,
    },
    Thinking {
        text: String,
        /// `true` while the model is still emitting thinking tokens.
        streaming: bool,
    },
    ToolCall {
        name: String,
        input: Value,
    },
    ToolResult {
        ok: bool,
        content: String,
        /// Pre-parsed JSON value to avoid re-parsing on every render
        /// frame. Parsed once at insertion time.
        parsed: Option<Value>,
    },
    Done,
    Error {
        message: String,
    },
    Divider,
}

impl ChatMessage {
    /// Render this message as a sequence of *logical* lines.
    pub fn to_lines(&self, theme: &dyn ChatPanelTheme) -> Vec<Line<'static>> {
        match self {
            ChatMessage::User { text } => render_user_message(text, theme),
            ChatMessage::Assistant { text, streaming } => {
                render_assistant_message(text, *streaming, theme)
            }
            ChatMessage::Thinking { text, streaming } => render_thinking(text, *streaming, theme),
            ChatMessage::ToolCall { name, input } => render_tool_call(name, input, theme),
            ChatMessage::ToolResult { ok, content, parsed } => {
                render_tool_result(*ok, content, parsed.as_ref(), theme)
            }
            ChatMessage::Done => vec![Line::from(Span::styled(
                format!("{}Agent completed", theme.done_prefix()),
                theme.success_style(),
            ))],
            ChatMessage::Error { message } => vec![Line::from(Span::styled(
                format!("{}{}", theme.error_prefix(), message),
                Style::default().fg(theme.tool_err()),
            ))],
            ChatMessage::Divider => vec![Line::from("")],
        }
    }

    /// Cheap upper-bound estimate of the number of `Line` objects that
    /// `to_lines()` would produce.
    pub fn estimate_lines(&self) -> usize {
        match self {
            ChatMessage::User { text } => {
                let line_count = text.lines().count();
                let truncated = line_count > MAX_USER_MESSAGE_LINES;
                let display = if truncated { MAX_USER_MESSAGE_LINES } else { line_count };
                let mut est = display;
                if truncated { est += 1; }
                est + 1
            }
            ChatMessage::Assistant { text, .. } => text.lines().count().max(1),
            ChatMessage::Thinking { text, .. } => {
                let line_count = text.lines().count();
                let mut est = 1;
                est += line_count.min(MAX_THINKING_LINES);
                if line_count > MAX_THINKING_LINES { est += 1; }
                est.max(2)
            }
            ChatMessage::ToolCall { name: _, input } => {
                1 + input.as_object().map_or(1, |m| m.len())
            }
            ChatMessage::ToolResult { ok: _, content: _, parsed } => {
                let mut est = 1;
                match parsed {
                    Some(Value::Object(map)) => { est += map.len(); }
                    Some(Value::Array(arr)) => { est += arr.len().min(MAX_ARRAY_ITEMS) + 1; }
                    _ => { est += 1; }
                }
                est
            }
            ChatMessage::Done => 1,
            ChatMessage::Error { .. } => 1,
            ChatMessage::Divider => 1,
        }
    }
}

fn render_user_message(text: &str, theme: &dyn ChatPanelTheme) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let text_lines: Vec<&str> = text.lines().collect();
    let total = text_lines.len();
    let truncated = total > MAX_USER_MESSAGE_LINES;
    let display_count = if truncated { MAX_USER_MESSAGE_LINES } else { total };

    for (i, line_text) in text_lines.iter().take(display_count).enumerate() {
        let prefix = if i == 0 {
            theme.user_prefix().to_string()
        } else {
            "    ".to_string()
        };
        lines.push(Line::from(Span::styled(
            format!("{}{}", prefix, line_text),
            theme.user_style(),
        )));
    }

    if truncated {
        lines.push(Line::from(Span::styled(
            format!("    … {} more lines (truncated)", total - MAX_USER_MESSAGE_LINES),
            Style::default().fg(theme.text_muted()),
        )));
    }

    lines
}

fn render_assistant_message(
    text: &str,
    streaming: bool,
    theme: &dyn ChatPanelTheme,
) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = text
        .lines()
        .map(|l| {
            Line::from(Span::styled(
                format!("{}{}", theme.assistant_prefix(), l),
                theme.assistant_style(),
            ))
        })
        .collect();

    if streaming {
        if let Some(last) = lines.last_mut() {
            last.spans.push(Span::styled(
                "█".to_string(),
                Style::default().fg(theme.spinner_color()),
            ));
        } else {
            lines.push(Line::from(Span::styled(
                format!("{}█", theme.assistant_prefix()),
                Style::default().fg(theme.spinner_color()),
            )));
        }
    }
    lines
}

fn render_thinking(text: &str, streaming: bool, theme: &dyn ChatPanelTheme) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(Span::styled(
        format!("{}Thinking:", theme.thinking_prefix()),
        theme.thinking_bold_style(),
    ))];
    for l in text.lines().take(MAX_THINKING_LINES) {
        lines.push(Line::from(Span::styled(
            format!("   {}", l),
            theme.thinking_style(),
        )));
    }
    let total = text.lines().count();
    if total > MAX_THINKING_LINES {
        lines.push(Line::from(Span::styled(
            format!("   … {} more lines", total - MAX_THINKING_LINES),
            Style::default().fg(theme.text_muted()),
        )));
    }
    if streaming {
        if let Some(last) = lines.last_mut() {
            last.spans.push(Span::styled(
                "█".to_string(),
                Style::default().fg(theme.spinner_color()),
            ));
        }
    }
    lines
}

fn render_tool_call(name: &str, input: &Value, theme: &dyn ChatPanelTheme) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(vec![
        Span::styled(theme.tool_prefix().to_string(), theme.tool_call_style()),
        Span::styled(name.to_string(), theme.tool_call_bold_style()),
    ])];
    if let Some(obj) = input.as_object() {
        let total = obj.len();
        for (key, val) in obj.iter().take(MAX_OBJECT_KEYS) {
            lines.push(Line::from(vec![
                Span::styled("    ".to_string(), Style::default()),
                Span::styled(
                    format!("{}: ", key),
                    Style::default().fg(theme.text_secondary()),
                ),
                Span::styled(
                    format_value(val),
                    Style::default().fg(theme.text_primary()),
                ),
            ]));
        }
        if total > MAX_OBJECT_KEYS {
            lines.push(Line::from(Span::styled(
                format!("    … and {} more keys", total - MAX_OBJECT_KEYS),
                Style::default().fg(theme.text_muted()),
            )));
        }
    } else if !input.is_null() {
        lines.push(Line::from(vec![
            Span::styled("    ".to_string(), Style::default()),
            Span::styled(
                truncate_str(&input.to_string(), MAX_TOOL_FIELD_CHARS),
                Style::default().fg(theme.text_primary()),
            ),
        ]));
    }
    lines
}

fn render_tool_result(
    ok: bool,
    content: &str,
    parsed: Option<&Value>,
    theme: &dyn ChatPanelTheme,
) -> Vec<Line<'static>> {
    let (prefix, color) = if ok {
        (theme.tool_ok_prefix().to_string(), theme.tool_ok())
    } else {
        (theme.tool_err_prefix().to_string(), theme.tool_err())
    };
    let style = Style::default().fg(color);
    let mut lines = vec![Line::from(Span::styled(prefix.clone(), style))];

    match parsed {
        Some(Value::Object(map)) => {
            let total = map.len();
            for (k, v) in map.iter().take(MAX_OBJECT_KEYS) {
                lines.push(Line::from(vec![
                    Span::styled("    ".to_string(), Style::default()),
                    Span::styled(
                        format!("{}: ", k),
                        Style::default().fg(theme.text_muted()),
                    ),
                    Span::styled(format_value(v), style),
                ]));
            }
            if total > MAX_OBJECT_KEYS {
                lines.push(Line::from(Span::styled(
                    format!("    … and {} more keys", total - MAX_OBJECT_KEYS),
                    Style::default().fg(theme.text_muted()),
                )));
            }
        }
        Some(Value::Array(arr)) => {
            for item in arr.iter().take(MAX_ARRAY_ITEMS) {
                let label = if let Some(s) = item.as_str() {
                    truncate_str(s, MAX_TOOL_FIELD_CHARS)
                } else {
                    format_value(item)
                };
                lines.push(Line::from(vec![
                    Span::styled("    ".to_string(), Style::default()),
                    Span::styled(format!("  • {}", label), style),
                ]));
            }
            if arr.len() > MAX_ARRAY_ITEMS {
                lines.push(Line::from(Span::styled(
                    format!("    … and {} more", arr.len() - MAX_ARRAY_ITEMS),
                    Style::default().fg(theme.text_muted()),
                )));
            }
        }
        Some(Value::String(s)) => {
            for l in s.lines().take(MAX_TOOL_RESULT_LINES) {
                lines.push(Line::from(vec![
                    Span::styled("    ".to_string(), Style::default()),
                    Span::styled(truncate_str(l, MAX_TOOL_FIELD_CHARS), style),
                ]));
            }
            let total_lines = s.lines().count();
            if total_lines > MAX_TOOL_RESULT_LINES {
                lines.push(Line::from(Span::styled(
                    format!(
                        "    … {} more lines (truncated)",
                        total_lines - MAX_TOOL_RESULT_LINES
                    ),
                    Style::default().fg(theme.text_muted()),
                )));
            }
        }
        Some(other) => {
            lines.push(Line::from(vec![
                Span::styled("    ".to_string(), Style::default()),
                Span::styled(
                    truncate_str(&other.to_string(), MAX_TOOL_FIELD_CHARS),
                    style,
                ),
            ]));
        }
        None => {
            let total_lines = content.lines().count();
            for l in content.lines().take(MAX_TOOL_RESULT_LINES) {
                lines.push(Line::from(vec![
                    Span::styled("    ".to_string(), Style::default()),
                    Span::styled(truncate_str(l, MAX_TOOL_FIELD_CHARS), style),
                ]));
            }
            if total_lines > MAX_TOOL_RESULT_LINES {
                lines.push(Line::from(Span::styled(
                    format!(
                        "    … {} more lines (truncated)",
                        total_lines - MAX_TOOL_RESULT_LINES
                    ),
                    Style::default().fg(theme.text_muted()),
                )));
            }
        }
    }
    lines
}

pub fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let end = s
        .char_indices()
        .take_while(|(i, _)| *i < max)
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(max);
    format!("{}…", &s[..end])
}

pub fn format_value(v: &Value) -> String {
    match v {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => truncate_str(s, MAX_TOOL_FIELD_CHARS),
        Value::Array(arr) => {
            if arr.is_empty() {
                "[]".to_string()
            } else if arr.len() == 1 {
                format!("[{}]", format_value(&arr[0]))
            } else {
                format!("[{} items]", arr.len())
            }
        }
        Value::Object(_) => "{…}".to_string(),
    }
}

#[cfg(test)]
mod render_tests {
    use super::*;

    fn theme() -> DefaultChatPanelTheme {
        DefaultChatPanelTheme
    }

    fn message_count(lines: &[Line<'_>]) -> usize {
        let mut total_more = 0usize;
        for line in lines {
            for span in &line.spans {
                if let Some(rest) = span.content.split('…').nth(1) {
                    if let Some(n) = rest
                        .trim_start()
                        .trim_start_matches(" and ")
                        .trim_start_matches("more lines (truncated)")
                        .split_whitespace()
                        .next()
                        .and_then(|s| s.parse::<usize>().ok())
                    {
                        total_more += n;
                    }
                }
            }
        }
        total_more
    }

    #[test]
    fn short_user_message_renders_in_full() {
        let text = "line a\nline b\nline c";
        let lines = render_user_message(text, &theme());
        assert_eq!(lines.len(), 3);
        assert!(lines[0].to_string().contains(theme().user_prefix()));
        assert!(lines[1].to_string().starts_with("    "));
    }

    #[test]
    fn exactly_threshold_lines_is_not_truncated() {
        let text = (1..=10)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let lines = render_user_message(&text, &theme());
        assert_eq!(lines.len(), 10);
        assert_eq!(message_count(&lines), 0);
    }

    #[test]
    fn over_threshold_user_message_is_folded() {
        let text = (1..=50)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let lines = render_user_message(&text, &theme());
        assert_eq!(lines.len(), 11);
        assert_eq!(message_count(&lines), 40);
    }

    #[test]
    fn assistant_message_is_never_folded() {
        let text = (1..=200)
            .map(|i| format!("reply {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let lines = render_assistant_message(&text, false, &theme());
        assert_eq!(lines.len(), 200);
    }

    #[test]
    fn empty_user_message_still_renders_nothing() {
        let lines = render_user_message("", &theme());
        assert_eq!(lines.len(), 0);
    }

    #[test]
    fn tool_call_folds_wide_object() {
        let mut obj = serde_json::Map::new();
        for i in 0..50 {
            obj.insert(format!("key_{i}"), Value::String(format!("v{i}")));
        }
        let input = Value::Object(obj);
        let lines = render_tool_call("kms_dummy", &input, &theme());
        assert_eq!(lines.len(), 1 + MAX_OBJECT_KEYS + 1);
        let joined: String = lines
            .iter()
            .map(|l| l.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("… and 40 more keys"));
    }

    #[test]
    fn tool_result_folds_long_string_field() {
        let long = "中".repeat(MAX_TOOL_FIELD_CHARS * 3);
        let parsed = serde_json::json!({ "content": long.clone() });
        let lines = render_tool_result(true, "", Some(&parsed), &theme());
        assert_eq!(lines.len(), 2);
        let rendered = lines[1].to_string();
        assert!(rendered.ends_with('…'), "expected truncated value, got {rendered:?}");
        let tail = rendered.split("content: ").nth(1).unwrap();
        assert!(tail.chars().count() <= MAX_TOOL_FIELD_CHARS + 1);
    }

    #[test]
    fn tool_result_string_folds_by_line_count() {
        let content: String = (1..=20)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let parsed = Value::String(content);
        let lines = render_tool_result(true, "", Some(&parsed), &theme());
        assert_eq!(lines.len(), 1 + MAX_TOOL_RESULT_LINES + 1);
        let joined = lines
            .iter()
            .map(|l| l.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("14 more lines (truncated)"));
    }

    #[test]
    fn tool_result_object_folds_wide() {
        let mut obj = serde_json::Map::new();
        for i in 0..25 {
            obj.insert(format!("k{i}"), Value::Number(i.into()));
        }
        let parsed = Value::Object(obj);
        let lines = render_tool_result(true, "", Some(&parsed), &theme());
        assert_eq!(lines.len(), 1 + MAX_OBJECT_KEYS + 1);
        let joined = lines
            .iter()
            .map(|l| l.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("… and 15 more keys"));
    }
}

#[cfg(test)]
mod estimate_tests {
    use super::*;

    fn theme() -> DefaultChatPanelTheme {
        DefaultChatPanelTheme
    }

    #[test]
    fn estimate_never_undercounts_assistant() {
        let msg = ChatMessage::Assistant {
            text: "line1\nline2\nline3".into(),
            streaming: false,
        };
        let actual = msg.to_lines(&theme()).len();
        assert!(
            msg.estimate_lines() >= actual,
            "estimate={} < actual={}",
            msg.estimate_lines(),
            actual
        );
    }

    #[test]
    fn estimate_never_undercounts_user() {
        let msg = ChatMessage::User {
            text: "line1\nline2".into(),
        };
        let actual = msg.to_lines(&theme()).len();
        assert!(
            msg.estimate_lines() >= actual,
            "estimate={} < actual={}",
            msg.estimate_lines(),
            actual
        );
    }

    #[test]
    fn estimate_simple_types() {
        assert_eq!(ChatMessage::Divider.estimate_lines(), 1);
        assert_eq!(ChatMessage::Done.estimate_lines(), 1);
        assert_eq!(
            ChatMessage::Error {
                message: "oops".into()
            }
            .estimate_lines(),
            1
        );
    }
}
