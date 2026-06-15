//! [`render_chat_panel`] — the chat panel's public render entry point.

use std::fmt::Debug;
use std::hash::Hash;

use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap};
use ratatui::Frame;

use super::state::ChatPanelState;
use super::theme::ChatPanelTheme;

/// Width of the scrollbar strip reserved on the right edge.
pub const SCROLLBAR_WIDTH: u16 = 1;

/// Render the chat conversation into `area`, with a vertical
/// scrollbar on the right edge.
pub fn render_chat_panel<K: Hash + Eq + Clone + Debug>(
    f: &mut Frame,
    state: &mut ChatPanelState<K>,
    theme: &dyn ChatPanelTheme,
    area: Rect,
) {
    let inner_height = area.height as usize;
    let inner_width = area.width;

    let msg_version = state.message_version();
    let cache_hit = matches!(
        state.cached_lines(),
        Some((ver, _lines)) if *ver == msg_version
    );
    let lines: Vec<Line<'static>> = if cache_hit {
        state.cached_lines().as_ref().unwrap().1.clone()
    } else {
        let lines: Vec<Line<'static>> = state
            .current_messages()
            .iter()
            .flat_map(|m| m.to_lines(theme))
            .collect();
        state.set_cached_lines(Some((msg_version, lines.clone())));
        lines
    };

    let total_visual_rows: usize = match state.cached_wrap() {
        Some((ver, w, rows)) if *ver == msg_version && *w == inner_width => *rows,
        _ => {
            let probe = Paragraph::new(lines.clone()).wrap(Wrap { trim: false });
            let rows = probe.line_count(inner_width);
            state.set_cached_wrap(Some((msg_version, inner_width, rows)));
            rows
        }
    };

    let max_scroll = total_visual_rows.saturating_sub(inner_height);
    let global_scroll = state.resolve_scroll(max_scroll);
    state.set_scroll((global_scroll.min(u16::MAX as usize)) as u16);

    let (paragraph_area, scrollbar_area) = if total_visual_rows > inner_height
        && inner_width > SCROLLBAR_WIDTH
    {
        let sb_width = SCROLLBAR_WIDTH;
        let paragraph_rect = Rect {
            x: area.x,
            y: area.y,
            width: area.width.saturating_sub(sb_width),
            height: area.height,
        };
        let scrollbar_rect = Rect {
            x: area.x + area.width.saturating_sub(sb_width),
            y: area.y,
            width: sb_width,
            height: area.height,
        };
        (paragraph_rect, Some(scrollbar_rect))
    } else {
        (area, None)
    };

    let total_rows_for_sb = if scrollbar_area.is_some() {
        let narrowed = paragraph_area.width;
        if narrowed != inner_width {
            let probe = Paragraph::new(lines.clone()).wrap(Wrap { trim: false });
            probe.line_count(narrowed)
        } else {
            total_visual_rows
        }
    } else {
        total_visual_rows
    };

    let paragraph = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((global_scroll.min(u16::MAX as usize) as u16, 0));
    f.render_widget(paragraph, paragraph_area);

    if let Some(sb_area) = scrollbar_area {
        let mut sb_state = ScrollbarState::new(total_rows_for_sb)
            .position(global_scroll.min(usize::from(u16::MAX)) as usize)
            .viewport_content_length(sb_area.height as usize);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(Some(" "))
            .thumb_symbol("█")
            .thumb_style(theme.scrollbar_thumb_style())
            .track_style(theme.scrollbar_track_style());
        f.render_stateful_widget(scrollbar, sb_area, &mut sb_state);
    }
}
