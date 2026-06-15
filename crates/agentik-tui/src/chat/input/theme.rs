//! Theme trait for the chat input / status row.

use ratatui::style::Color;

/// Colors the chat input renderer reads while laying out the
/// bottom status / prompt row.
pub trait ChatInputTheme {
    fn text_muted(&self) -> Color;
    fn spinner_color(&self) -> Color;
    fn input_bg(&self) -> Color;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultChatInputTheme;

impl ChatInputTheme for DefaultChatInputTheme {
    fn text_muted(&self) -> Color {
        Color::DarkGray
    }
    fn spinner_color(&self) -> Color {
        Color::Yellow
    }
    fn input_bg(&self) -> Color {
        Color::DarkGray
    }
}
