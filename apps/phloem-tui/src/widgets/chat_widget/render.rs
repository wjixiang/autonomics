use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use tui_markdown::from_str_with_options;

use crate::state::{ChatLine, TurnUsage};
use crate::widgets::status_bar::format_tokens;

use super::style::MD_OPTIONS;
use super::table::{is_table_separator, render_table_lines};

/// Format a turn's token usage as a compact dim line, e.g. "↑1.2k ↓340 cache:800".
fn format_turn_usage(u: TurnUsage) -> String {
    let mut parts = Vec::new();
    if let Some(inp) = u.input_tokens {
        parts.push(format!("↑{}", format_tokens(inp)));
    }
    parts.push(format!("↓{}", format_tokens(u.output_tokens)));
    if let Some(cache) = u.cache_read_input_tokens {
        if cache > 0 {
            parts.push(format!("cache:{}", format_tokens(cache)));
        }
    }
    parts.join("  ")
}

/// Render assistant message text with markdown support.
/// Splits the text into non-table segments (rendered via `tui-markdown`)
/// and table segments (rendered with box-drawing characters).
///
/// Non-table segments borrow from `text` directly (no local String allocation),
/// so lifetimes flow naturally. Table segments produce owned `Line<'static>`.
fn render_assistant_text<'a>(text: &'a str, available_width: usize, out: &mut Vec<Line<'a>>) {
    let text_lines: Vec<&str> = text.lines().collect();
    if text_lines.is_empty() {
        return;
    }

    let mut pos = 0;
    while pos < text_lines.len() {
        let trimmed = text_lines[pos].trim();
        let is_table = trimmed.starts_with('|')
            && trimmed.ends_with('|')
            && pos + 1 < text_lines.len()
            && is_table_separator(text_lines[pos + 1].trim());

        if is_table {
            let start = pos;
            while pos < text_lines.len() && text_lines[pos].trim().starts_with('|') {
                pos += 1;
            }
            // Table rendering produces owned Lines ('static), coerces to '_
            out.extend(render_table_lines(&text_lines[start..pos], available_width));
        } else {
            let start = pos;
            while pos < text_lines.len() {
                let t = text_lines[pos].trim();
                let table_ahead = t.starts_with('|')
                    && t.ends_with('|')
                    && pos + 1 < text_lines.len()
                    && is_table_separator(text_lines[pos + 1].trim());
                if table_ahead {
                    break;
                }
                pos += 1;
            }
            if pos == start {
                // No non-table lines consumed — skip to avoid empty segment
                continue;
            }
            // Compute byte range within the original text for this segment.
            let seg_start = text_lines[start].as_ptr() as usize - text.as_ptr() as usize;
            let seg_end = text_lines[pos - 1].as_ptr() as usize + text_lines[pos - 1].len()
                - text.as_ptr() as usize;
            let segment = &text[seg_start..seg_end];
            let md_text = from_str_with_options(segment, &MD_OPTIONS);
            out.extend(md_text.lines);
        }
    }
}

/// Convert borrowed `Line` / `Span` data into fully owned `Line<'static>`.
fn into_owned_lines(lines: Vec<Line<'_>>) -> Vec<Line<'static>> {
    lines
        .into_iter()
        .map(|line| {
            Line::from(
                line.spans
                    .into_iter()
                    .map(|span| Span::styled(
                        span.content.into_owned(),
                        span.style,
                    ))
                    .collect::<Vec<_>>(),
            )
        })
        .collect()
}

/// Render a single `ChatLine` into one or more owned `Line<'static>`.
pub(crate) fn render_line_owned(msg: &ChatLine, area: Rect) -> Vec<Line<'static>> {
    match msg {
        ChatLine::User(text) => {
            let mut lines = vec![Line::from(Span::styled(
                "You",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ))];
            for line in text.lines() {
                lines.push(Line::from(Span::styled(
                    line.to_string(),
                    Style::default().fg(Color::Cyan),
                )));
            }
            lines
        }
        ChatLine::Assistant { text, usage } => {
            let mut lines: Vec<Line<'_>> = vec![Line::from(Span::styled(
                "Assistant",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ))];
            render_assistant_text(text, area.width as usize, &mut lines);
            if let Some(u) = usage {
                lines.push(Line::from(Span::styled(
                    format_turn_usage(*u),
                    Style::default().fg(Color::DarkGray),
                )));
            }
            into_owned_lines(lines)
        }
        ChatLine::Thinking(text) => {
            let mut lines = Vec::new();
            let mut first = true;
            for line in text.lines() {
                let prefix = if first { "💭 " } else { "   " };
                lines.push(Line::from(Span::styled(
                    format!("{prefix}{line}"),
                    Style::default().fg(Color::DarkGray),
                )));
                first = false;
            }
            lines
        }
        ChatLine::ToolCall { name, input } => {
            let mut lines = vec![Line::from(Span::styled(
                format!("🔧 Calling: {}", name),
                Style::default().fg(Color::Yellow),
            ))];
            if !input.is_empty() {
                // Render each input parameter on its own indented line.
                for line in input.lines() {
                    lines.push(Line::from(Span::styled(
                        format!("   {}", line),
                        Style::default().fg(Color::DarkGray),
                    )));
                }
            }
            lines
        }
        ChatLine::ToolBackground { id: _id, name } => {
            vec![Line::from(Span::styled(
                format!("⏳ Calling: {} (running in background)", name),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::DIM),
            ))]
        }
        ChatLine::ToolResult { ok, content } => {
            let icon = if *ok { "✓" } else { "✗" };
            let color = if *ok { Color::Green } else { Color::Red };
            let mut lines = Vec::new();
            let mut first = true;
            for line in content.lines() {
                let prefix = if first {
                    format!("{icon} ")
                } else {
                    "  ".to_string()
                };
                lines.push(Line::from(Span::styled(
                    format!("{prefix}{line}"),
                    Style::default().fg(color),
                )));
                first = false;
            }
            lines
        }
        ChatLine::Error(text) => {
            let mut lines = Vec::new();
            let mut first = true;
            for line in text.lines() {
                let prefix = if first { "✗ Error: " } else { "         " };
                lines.push(Line::from(Span::styled(
                    format!("{prefix}{line}"),
                    Style::default().fg(Color::Red),
                )));
                first = false;
            }
            lines
        }
        ChatLine::Separator => {
            vec![Line::from(Span::styled(
                "─".repeat(area.width as usize),
                Style::default().fg(Color::DarkGray),
            ))]
        }
    }
}
