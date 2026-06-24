use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::Rect,
    prelude::{StatefulWidget, Widget},
    style::{Color, Style},
    widgets::Block,
};
use ratatui_textarea::TextArea;

// ── Single-line text input (used by the config tab forms) ──────

/// State for a single-line text input.
#[derive(Debug, Default)]
pub struct InputState {
    content: String,
    cursor: usize,
}

impl InputState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn value(&self) -> &str {
        &self.content
    }

    pub fn is_empty(&self) -> bool {
        self.content.is_empty()
    }

    pub fn clear(&mut self) {
        self.content.clear();
        self.cursor = 0;
    }

    /// Insert a character at the cursor position.
    pub fn insert(&mut self, ch: char) {
        self.content.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
    }

    /// Delete the character before the cursor.
    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            let prev = self.content[..self.cursor]
                .chars()
                .last()
                .map(|c| c.len_utf8())
                .unwrap_or(1);
            self.cursor -= prev;
            self.content.remove(self.cursor);
        }
    }

    /// Move cursor left.
    pub fn cursor_left(&mut self) {
        if self.cursor > 0 {
            self.cursor -= self.content[..self.cursor]
                .chars()
                .last()
                .map(|c| c.len_utf8())
                .unwrap_or(1);
        }
    }

    /// Move cursor right.
    pub fn cursor_right(&mut self) {
        if self.cursor < self.content.len() {
            self.cursor += self.content[self.cursor..]
                .chars()
                .next()
                .map(|c| c.len_utf8())
                .unwrap_or(1);
        }
    }

    /// Move cursor to start.
    pub fn cursor_home(&mut self) {
        self.cursor = 0;
    }

    /// Move cursor to end.
    pub fn cursor_end(&mut self) {
        self.cursor = self.content.len();
    }

    /// Handle a key event. Returns true if the key was consumed.
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char(c) => {
                self.insert(c);
                true
            }
            KeyCode::Backspace => {
                self.backspace();
                true
            }
            KeyCode::Left => {
                self.cursor_left();
                true
            }
            KeyCode::Right => {
                self.cursor_right();
                true
            }
            KeyCode::Home => {
                self.cursor_home();
                true
            }
            KeyCode::End => {
                self.cursor_end();
                true
            }
            KeyCode::Delete => {
                if self.cursor < self.content.len() {
                    self.content.remove(self.cursor);
                }
                true
            }
            _ => false,
        }
    }
}

// ── Multi-line textarea input (used by the agent chat) ─────────

/// Wrapper around `ratatui_textarea::TextArea` for the chat input area.
///
/// Uses `TextArea<'static>` so the struct carries no lifetime and can be
/// stored directly in state (e.g. inside `AgentTabState`).
#[derive(Debug)]
pub struct InputArea {
    textarea: TextArea<'static>,
}

impl InputArea {
    pub fn new() -> Self {
        let mut textarea = TextArea::default();
        textarea.set_placeholder_text("Type a message...");
        textarea.set_placeholder_style(Style::default().fg(Color::DarkGray));
        // No block set — the InputWidget renders its own bordered block around us.
        Self { textarea }
    }

    /// Get the current text content as a single string.
    pub fn value(&self) -> String {
        self.textarea.lines().join("\n")
    }

    /// Check if the textarea is empty.
    pub fn is_empty(&self) -> bool {
        self.textarea.is_empty()
    }

    /// Clear all content.
    pub fn clear(&mut self) {
        self.textarea.clear();
    }

    /// Handle a key event. Returns true if the text content was modified.
    /// Enter and Escape are intercepted (not forwarded to the textarea) since
    /// the caller handles them for message sending / mode switching.
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Enter | KeyCode::Esc => false,
            _ => self.textarea.input(key),
        }
    }

    /// Insert a paste string at the cursor position.
    pub fn insert_str(&mut self, s: &str) {
        self.textarea.insert_str(s);
    }

    /// Set placeholder text (used for disabled / running state).
    pub fn set_placeholder(&mut self, text: &str) {
        self.textarea.set_placeholder_text(text);
    }

    /// Borrow the inner textarea for rendering.
    pub fn textarea(&self) -> &TextArea<'_> {
        &self.textarea
    }
}

impl Default for InputArea {
    fn default() -> Self {
        Self::new()
    }
}

/// State for [`InputWidget`].
pub struct InputWidgetState<'a> {
    pub input: &'a mut InputArea,
}

/// Composite widget: bordered input area backed by ratatui-textarea.
pub struct InputWidget<'a> {
    pub disabled: bool,
    pub title: &'a str,
    pub placeholder: &'a str,
}

impl<'a> StatefulWidget for InputWidget<'a> {
    type State = InputWidgetState<'a>;

    fn render(self, area: Rect, buf: &mut ratatui::prelude::Buffer, state: &mut Self::State) {
        let style = if self.disabled {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default()
        };

        let block = Block::bordered()
            .title(format!(" {} ", self.title))
            .border_style(style);
        let inner = block.inner(area);
        block.render(area, buf);

        state.input.set_placeholder(self.placeholder);
        state.input.textarea().render(inner, buf);
    }
}
