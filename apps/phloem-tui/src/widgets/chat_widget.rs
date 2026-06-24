use std::sync::LazyLock;

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    prelude::{StatefulWidget, Widget},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
};
use tui_markdown::{from_str_with_options, Options, StyleSheet};

use crate::state::ChatLine;

/// Dark-theme stylesheet for markdown rendering in the chat widget.
#[derive(Debug, Clone, Copy, Default)]
struct PhloemStyleSheet;

impl StyleSheet for PhloemStyleSheet {
    fn heading(&self, level: u8) -> Style {
        match level {
            1 => Style::default()
                .fg(Color::Rgb(220, 220, 255))
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            2 => Style::default()
                .fg(Color::Rgb(180, 180, 255))
                .add_modifier(Modifier::BOLD),
            3 => Style::default()
                .fg(Color::Rgb(160, 160, 240))
                .add_modifier(Modifier::BOLD),
            _ => Style::default()
                .fg(Color::Rgb(140, 140, 220))
                .add_modifier(Modifier::ITALIC),
        }
    }

    fn code(&self) -> Style {
        Style::default()
            .fg(Color::Rgb(200, 200, 200))
            .bg(Color::Rgb(40, 40, 40))
    }

    fn link(&self) -> Style {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::UNDERLINED)
    }

    fn blockquote(&self) -> Style {
        Style::default()
            .fg(Color::Rgb(180, 180, 100))
            .add_modifier(Modifier::ITALIC)
    }

    fn heading_meta(&self) -> Style {
        Style::default().fg(Color::DarkGray)
    }

    fn metadata_block(&self) -> Style {
        Style::default().fg(Color::Rgb(180, 180, 160))
    }
}

static MD_OPTIONS: LazyLock<Options<PhloemStyleSheet>> =
    LazyLock::new(|| Options::new(PhloemStyleSheet));

/// State for [`ChatWidget`].
pub struct ChatWidgetState {
    pub total_lines: usize,
    pub viewport_height: u16,
    pub scroll_offset: usize,
}

impl ChatWidgetState {
    pub fn new(scroll_offset: usize) -> Self {
        Self {
            total_lines: 0,
            viewport_height: 0,
            scroll_offset,
        }
    }
}

/// Chat message list with scrolling and scrollbar support.
pub struct ChatWidget<'a> {
    pub messages: &'a [ChatLine],
}

impl StatefulWidget for ChatWidget<'_> {
    type State = ChatWidgetState;

    fn render(self, area: Rect, buf: &mut ratatui::prelude::Buffer, state: &mut Self::State) {
        state.viewport_height = area.height;

        let lines: Vec<Line<'_>> = self.messages.iter().flat_map(render_line).collect();
        let paragraph = Paragraph::new(lines).wrap(Wrap { trim: true });

        state.total_lines = paragraph.line_count(area.width);

        let paragraph = paragraph.scroll((state.scroll_offset as u16, 0));
        paragraph.render(area, buf);

        // Render scrollbar overlaid on the right edge of the chat area
        if state.total_lines > area.height as usize {
            let mut scrollbar_state = ScrollbarState::new(state.total_lines - area.height as usize)
                .position(state.scroll_offset)
                .viewport_content_length(area.height as usize);
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .thumb_style(Style::default().fg(Color::DarkGray))
                .track_style(Style::default().fg(Color::Rgb(40, 40, 40)));
            scrollbar.render(area, buf, &mut scrollbar_state);
        }
    }
}

/// Strip the leading and trailing `|` from a pipe-delimited table line,
/// returning the inner content.
///
/// Returns `None` unless `s` is at least two characters and both starts and
/// ends with `|`. This guards the slice `s[1..len-1]` against a lone `|`
/// (length 1), which would otherwise panic with an invalid range
/// (`[1..0]`).
fn pipe_inner<'a>(s: &'a str) -> Option<&'a str> {
    if s.len() < 2 || !s.starts_with('|') || !s.ends_with('|') {
        return None;
    }
    Some(&s[1..s.len() - 1])
}

/// Check if a line is a markdown table separator (e.g. `| --- | --- |`).
fn is_table_separator(line: &str) -> bool {
    let trimmed = line.trim();
    match pipe_inner(trimmed) {
        Some(inner) => inner
            .chars()
            .all(|c| c == '-' || c == '|' || c == ':' || c == ' '),
        None => false,
    }
}

/// Render a markdown table as styled `Line`s with box-drawing characters.
fn render_table_lines(table_lines: &[&str]) -> Vec<Line<'static>> {
    let mut rows: Vec<Vec<&str>> = Vec::new();

    for line in table_lines {
        let trimmed = line.trim();
        if is_table_separator(trimmed) {
            continue;
        }
        if let Some(inner) = pipe_inner(trimmed) {
            let cells: Vec<&str> = inner.split('|').map(|s| s.trim()).collect();
            rows.push(cells);
        }
    }

    if rows.is_empty() {
        return table_lines
            .iter()
            .map(|&l| Line::from(l.to_string()))
            .collect();
    }

    let num_cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    let mut col_widths: Vec<usize> = vec![3; num_cols];
    for row in &rows {
        for (j, cell) in row.iter().enumerate().take(num_cols) {
            col_widths[j] = col_widths[j].max(cell.len() + 2);
        }
    }

    let bg_color = Color::Rgb(40, 40, 40);
    let border = Style::default().fg(Color::Rgb(100, 100, 120)).bg(bg_color);
    let header_style = Style::default()
        .fg(Color::Rgb(180, 180, 255))
        .bg(bg_color)
        .add_modifier(Modifier::BOLD);
    let cell_style = Style::default().fg(Color::Rgb(200, 200, 200)).bg(bg_color);

    let mut result = Vec::new();

    // Helper: push a horizontal border line (e.g. ┌──┬──┐)
    let make_border = |left: char, mid: char, right: char| -> Line<'static> {
        let mut spans = vec![Span::styled(left.to_string(), border)];
        for (j, w) in col_widths.iter().enumerate() {
            spans.push(Span::styled("─".repeat(*w), border));
            if j + 1 < num_cols {
                spans.push(Span::styled(mid.to_string(), border));
            }
        }
        spans.push(Span::styled(right.to_string(), border));
        Line::from(spans)
    };

    result.push(make_border('┌', '┬', '┐'));

    for (ri, row) in rows.iter().enumerate() {
        let style = if ri == 0 { header_style } else { cell_style };
        let mut spans = vec![Span::styled("│".to_string(), border)];
        for (j, cell_content) in row.iter().enumerate().take(num_cols) {
            let pad = col_widths[j] - cell_content.len();
            spans.push(Span::styled(
                format!(" {}{:<width$}", cell_content, "", width = pad - 1),
                style,
            ));
            spans.push(Span::styled("│".to_string(), border));
        }
        result.push(Line::from(spans));

        if ri == 0 {
            result.push(make_border('├', '┼', '┤'));
        }
    }

    result.push(make_border('└', '┴', '┘'));
    result
}

/// Render assistant message text with markdown support.
/// Splits the text into non-table segments (rendered via `tui-markdown`)
/// and table segments (rendered with box-drawing characters).
///
/// Non-table segments borrow from `text` directly (no local String allocation),
/// so lifetimes flow naturally. Table segments produce owned `Line<'static>`.
fn render_assistant_text<'a>(text: &'a str, out: &mut Vec<Line<'a>>) {
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
            out.extend(render_table_lines(&text_lines[start..pos]));
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

/// Render a single `ChatLine` into one or more `Line`s.
fn render_line(msg: &ChatLine) -> Vec<Line<'_>> {
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
        ChatLine::Assistant(text) => {
            let mut lines = vec![Line::from(Span::styled(
                "Assistant",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ))];
            render_assistant_text(text, &mut lines);
            lines
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
                "─".repeat(40),
                Style::default().fg(Color::DarkGray),
            ))]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipe_inner_handles_lone_pipe_without_panicking() {
        // Regression: a bare `|` (length 1) previously made
        // `trimmed[1..trimmed.len() - 1]` slice `[1..0]` and panic.
        assert_eq!(pipe_inner("|"), None);
        assert_eq!(is_table_separator("|"), false);

        // Normal cases.
        assert_eq!(pipe_inner("| a | b |"), Some(" a | b "));
        assert!(is_table_separator("| --- | --- |"));
        assert!(!is_table_separator("| a | b |"));

        // Empty-content pipe pair is still a valid inner slice.
        assert_eq!(pipe_inner("||"), Some(""));
    }
}
