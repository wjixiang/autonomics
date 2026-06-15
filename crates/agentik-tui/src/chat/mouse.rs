//! Mouse-event handling for the chat panel.

use std::fmt::Debug;
use std::hash::Hash;

use ratatui::layout::Rect;

use super::state::ChatPanelState;

/// What happened when a mouse event was processed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatMouseOutcome {
    Handled,
    Ignored,
}

/// Handle a mouse event for the chat panel.
pub fn handle_chat_mouse_event<K: Hash + Eq + Clone + Debug>(
    state: &mut ChatPanelState<K>,
    mouse_column: u16,
    mouse_row: u16,
    mouse_kind: MouseEventKind,
    chat_area: Rect,
    scroll_amount: u16,
    total_rows: usize,
) -> ChatMouseOutcome {
    if !rect_contains(chat_area, mouse_column, mouse_row) {
        return ChatMouseOutcome::Ignored;
    }

    let on_scrollbar = mouse_column == chat_area.x + chat_area.width.saturating_sub(1);

    match mouse_kind {
        MouseEventKind::ScrollUp => {
            state.scroll_up(scroll_amount);
            ChatMouseOutcome::Handled
        }
        MouseEventKind::ScrollDown => {
            state.scroll_down(scroll_amount);
            ChatMouseOutcome::Handled
        }
        MouseEventKind::Down(MouseButton::Left) if on_scrollbar => {
            if total_rows == 0 || chat_area.height == 0 {
                return ChatMouseOutcome::Handled;
            }
            let viewport = chat_area.height as usize;
            let max_scroll = total_rows.saturating_sub(viewport);
            if max_scroll == 0 {
                return ChatMouseOutcome::Handled;
            }
            let rel = (mouse_row - chat_area.y) as usize;
            let target = rel.saturating_mul(total_rows) / chat_area.height as usize;
            let target = target.min(max_scroll);
            state.disable_auto_scroll();
            state.set_scroll(target as u16);
            ChatMouseOutcome::Handled
        }
        _ => ChatMouseOutcome::Ignored,
    }
}

fn rect_contains(r: Rect, col: u16, row: u16) -> bool {
    col >= r.x && col < r.x.saturating_add(r.width) && row >= r.y && row < r.y.saturating_add(r.height)
}

/// Subset of `crossterm::event::MouseEventKind` the chat panel reacts to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseEventKind {
    ScrollUp,
    ScrollDown,
    Down(MouseButton),
    Other,
}

/// Subset of `crossterm::event::MouseButton` the chat panel reacts to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Other,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::ChatMessage;

    fn state() -> ChatPanelState<u8> {
        let mut s = ChatPanelState::new(0);
        s.insert_history(0, vec![ChatMessage::Divider]);
        for i in 0..200 {
            s.push_message(ChatMessage::Assistant {
                text: format!("line {i}"),
                streaming: false,
            });
        }
        s
    }

    fn area() -> Rect {
        Rect {
            x: 10,
            y: 5,
            width: 40,
            height: 20,
        }
    }

    #[test]
    fn scroll_up_disables_auto_scroll_and_moves() {
        let mut s = state();
        s.disable_auto_scroll();
        s.set_scroll(5);
        handle_chat_mouse_event(&mut s, 20, 10, MouseEventKind::ScrollUp, area(), 1, 200);
        assert!(!s.auto_scroll());
        assert_eq!(s.scroll(), 4);
    }

    #[test]
    fn scroll_up_at_top_is_a_no_op() {
        let mut s = state();
        s.disable_auto_scroll();
        s.set_scroll(0);
        handle_chat_mouse_event(&mut s, 20, 10, MouseEventKind::ScrollUp, area(), 1, 200);
        assert!(!s.auto_scroll());
        assert_eq!(s.scroll(), 0);
    }

    #[test]
    fn scroll_down_clamps_to_max() {
        let mut s = state();
        s.disable_auto_scroll();
        s.set_scroll(0);
        handle_chat_mouse_event(
            &mut s,
            20,
            10,
            MouseEventKind::ScrollDown,
            area(),
            5,
            200,
        );
        assert_eq!(s.scroll(), 5);
    }

    #[test]
    fn event_outside_chat_area_is_ignored() {
        let mut s = state();
        let v0 = s.scroll();
        let auto0 = s.auto_scroll();
        let outcome = handle_chat_mouse_event(
            &mut s,
            0,
            0,
            MouseEventKind::ScrollUp,
            area(),
            3,
            200,
        );
        assert_eq!(outcome, ChatMouseOutcome::Ignored);
        assert_eq!(s.scroll(), v0);
        assert_eq!(s.auto_scroll(), auto0);
    }

    #[test]
    fn click_on_scrollbar_jumps_to_position() {
        let mut s = state();
        s.disable_auto_scroll();
        s.set_scroll(0);
        let outcome = handle_chat_mouse_event(
            &mut s,
            49,
            15,
            MouseEventKind::Down(MouseButton::Left),
            area(),
            1,
            200,
        );
        assert_eq!(outcome, ChatMouseOutcome::Handled);
        assert_eq!(s.scroll(), 100);
    }

    #[test]
    fn click_outside_scrollbar_strip_is_ignored() {
        let mut s = state();
        let v0 = s.scroll();
        let outcome = handle_chat_mouse_event(
            &mut s,
            20,
            15,
            MouseEventKind::Down(MouseButton::Left),
            area(),
            1,
            200,
        );
        assert_eq!(outcome, ChatMouseOutcome::Ignored);
        assert_eq!(s.scroll(), v0);
    }

    #[test]
    fn non_wheel_non_scrollbar_event_is_ignored() {
        let mut s = state();
        let v0 = s.scroll();
        let outcome = handle_chat_mouse_event(
            &mut s,
            20,
            10,
            MouseEventKind::Down(MouseButton::Other),
            area(),
            1,
            200,
        );
        assert_eq!(outcome, ChatMouseOutcome::Ignored);
        assert_eq!(s.scroll(), v0);
    }

    #[test]
    fn click_on_scrollbar_with_empty_history_is_no_op() {
        let mut s = ChatPanelState::new(0);
        s.insert_history(0, vec![ChatMessage::Divider]);
        let outcome = handle_chat_mouse_event(
            &mut s,
            49,
            10,
            MouseEventKind::Down(MouseButton::Left),
            area(),
            1,
            0,
        );
        assert_eq!(outcome, ChatMouseOutcome::Handled);
        assert_eq!(s.scroll(), 0);
    }
}
