//! [`render_chat_input`] — frame-side renderer for the chat input / status row.

use std::fmt::Debug;
use std::hash::Hash;

use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use super::{build_status_line, ChatInputStatus, ChatInputTheme};
use crate::chat::state::ChatPanelState;

/// Render the chat input / status row into `area`.
pub fn render_chat_input<K: Hash + Eq + Clone + Debug>(
    f: &mut Frame,
    state: &mut ChatPanelState<K>,
    status: &ChatInputStatus,
    kind_label: &str,
    spinner_tick: usize,
    theme: &dyn ChatInputTheme,
    area: Rect,
    focused: bool,
) {
    let line: Line<'static> = build_status_line(
        status,
        state.input_text(),
        kind_label,
        spinner_tick,
        theme,
    );

    let bg = if matches!(status, ChatInputStatus::InputActive) {
        theme.input_bg()
    } else {
        Color::Reset
    };

    let paragraph = Paragraph::new(line.clone()).style(Style::default().bg(bg));
    f.render_widget(paragraph, area);

    if focused && matches!(status, ChatInputStatus::InputActive) {
        let cursor_x = area.x.saturating_add(line.width() as u16);
        let cursor_y = area.y;
        f.set_cursor_position((cursor_x, cursor_y));
    }
}
