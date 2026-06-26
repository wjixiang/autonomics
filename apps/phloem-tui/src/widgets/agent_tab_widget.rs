use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    prelude::{StatefulWidget, Widget},
    widgets::{Block, Padding},
};

use crate::state::{AgentTabState, InputMode};
use crate::widgets::{
    chat_widget::{ChatWidget, ChatWidgetState},
    input_area::{InputWidget, InputWidgetState},
    status_bar::StatusBar,
    tool_exec_widget::ToolExecWidget,
};

/// Composite widget that renders the entire Agent tab: status bar, chat area with
/// scrollbar, and input area.
pub struct AgentTabWidget<'a> {
    pub state: &'a mut AgentTabState,
}

impl Widget for AgentTabWidget<'_> {
    fn render(self, area: Rect, buf: &mut ratatui::prelude::Buffer) {
        let task_count = self.state.tool_tasks.len() as u16;
        let task_constraint = if task_count > 0 {
            Constraint::Length(task_count + 2) // +2 for border
        } else {
            Constraint::Length(0)
        };

        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // StatusBar
                task_constraint,       // ToolExecWidget (0 when empty)
                Constraint::Min(5),    // Chat
                Constraint::Length(3), // Input
            ])
            .split(area);

        let ts = &mut *self.state;

        // ── StatusBar ──
        let status_bar = StatusBar {
            status: &ts.status,
            input_tokens: ts.input_tokens,
            output_tokens: ts.output_tokens,
        };
        status_bar.render(layout[0], buf);

        // ── Tool execution panel ──
        if task_count > 0 {
            let tool_widget = ToolExecWidget {
                tasks: &ts.tool_tasks,
            };
            tool_widget.render(layout[1], buf);
        }

        // ── Chat area ──
        let chat_area = layout[2];
        let chat_block = Block::default().padding(Padding::new(2, 2, 2, 2));
        let chat_inner_area = chat_block.inner(chat_area);

        let viewport_height = chat_inner_area.height;
        ts.clamp_scroll(viewport_height);

        let width = chat_inner_area.width;
        let cache_hit = ts.cached_version == ts.messages_version
            && ts.cached_width == width
            && !ts.cached_lines.is_empty();

        let cached = if cache_hit {
            Some(ts.cached_lines.clone())
        } else {
            None
        };

        let mut chat_state = ChatWidgetState::new(ts.scroll_offset);
        let chat_widget = ChatWidget {
            messages: &ts.messages,
            cached_lines: cached,
        };

        chat_widget.render(chat_inner_area, buf, &mut chat_state);
        ts.content_line_count = chat_state.total_lines;

        // Update cache: store freshly rendered lines if this was a cache miss.
        if let Some(fresh) = chat_state.rendered_lines.take() {
            ts.cached_lines = fresh;
            ts.cached_width = width;
            ts.cached_version = ts.messages_version;
        }

        // ── Input area ──
        let running = ts.status != crate::state::AgentStatus::Idle;
        let title: &str = match (running, ts.input_mode) {
            (true, _) => " ■ input (running) ",
            (false, InputMode::Browse) => "▏browse (Enter=edit) ",
            (false, InputMode::Input) => " > input ",
        };

        let input_widget = InputWidget {
            disabled: running,
            title,
            placeholder: "Type a message...",
        };
        let mut input_state = InputWidgetState {
            input: &mut ts.input,
        };
        input_widget.render(layout[3], buf, &mut input_state);
    }
}
