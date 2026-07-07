mod md_highlight;
mod md_math;
mod md_mermaid;
mod md_renderer;
mod md_table;
mod md_theme;
pub(crate) mod render;
pub(crate) mod text_layout;

use ratatui::{
    layout::Rect,
    prelude::{StatefulWidget, Widget},
    style::{Color, Style},
    text::Line,
    widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
};

use crate::state::ChatLine;

/// State for [`ChatWidget`].
pub struct ChatWidgetState {
    pub total_lines: usize,
    pub viewport_height: u16,
    pub scroll_offset: usize,
    /// Lines rendered during a fresh (non-cached) render, available for caching by the caller.
    pub rendered_lines: Option<Vec<Line<'static>>>,
}

impl ChatWidgetState {
    pub fn new(scroll_offset: usize) -> Self {
        Self {
            total_lines: 0,
            viewport_height: 0,
            scroll_offset,
            rendered_lines: None,
        }
    }
}

/// Chat message list with scrolling and scrollbar support.
pub struct ChatWidget<'a> {
    pub messages: &'a [ChatLine],
    /// Pre-rendered lines from a previous frame (same messages, same width).
    /// When `Some`, the expensive markdown-parse / layout pass is skipped.
    pub cached_lines: Option<Vec<Line<'static>>>,
}

impl StatefulWidget for ChatWidget<'_> {
    type State = ChatWidgetState;

    fn render(self, area: Rect, buf: &mut ratatui::prelude::Buffer, state: &mut Self::State) {
        state.viewport_height = area.height;

        let needs_render = self.cached_lines.is_none();

        let lines: Vec<Line<'static>> = if let Some(cached) = self.cached_lines {
            cached
        } else {
            self.messages
                .iter()
                .flat_map(|f| render::render_line_owned(f, area))
                .collect()
        };

        // If we just freshly rendered, store back for next frame caching.
        if needs_render {
            state.rendered_lines = Some(lines.clone());
        }

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
