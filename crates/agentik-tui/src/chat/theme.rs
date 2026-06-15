//! Theme trait for the chat panel.

use ratatui::style::{Color, Modifier, Style};

/// Colors, styles, and message prefixes the chat panel renderer
/// reads while laying out a conversation history.
pub trait ChatPanelTheme {
    fn text_primary(&self) -> Color;
    fn text_secondary(&self) -> Color;
    fn text_muted(&self) -> Color;
    fn spinner_color(&self) -> Color;
    fn tool_ok(&self) -> Color;
    fn tool_err(&self) -> Color;
    fn user_style(&self) -> Style;
    fn assistant_style(&self) -> Style;
    fn thinking_style(&self) -> Style;
    fn thinking_bold_style(&self) -> Style;
    fn tool_call_style(&self) -> Style;
    fn tool_call_bold_style(&self) -> Style;
    fn success_style(&self) -> Style;
    fn scrollbar_thumb_style(&self) -> Style {
        Style::default().fg(self.text_muted())
    }
    fn scrollbar_track_style(&self) -> Style {
        Style::default()
    }
    fn user_prefix(&self) -> &'static str;
    fn assistant_prefix(&self) -> &'static str;
    fn thinking_prefix(&self) -> &'static str;
    fn tool_prefix(&self) -> &'static str;
    fn tool_ok_prefix(&self) -> &'static str;
    fn tool_err_prefix(&self) -> &'static str;
    fn done_prefix(&self) -> &'static str;
    fn error_prefix(&self) -> &'static str;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultChatPanelTheme;

impl ChatPanelTheme for DefaultChatPanelTheme {
    fn text_primary(&self) -> Color {
        Color::White
    }
    fn text_secondary(&self) -> Color {
        Color::Gray
    }
    fn text_muted(&self) -> Color {
        Color::DarkGray
    }
    fn spinner_color(&self) -> Color {
        Color::Yellow
    }
    fn tool_ok(&self) -> Color {
        Color::Green
    }
    fn tool_err(&self) -> Color {
        Color::Red
    }
    fn user_style(&self) -> Style {
        Style::default().fg(Color::Yellow)
    }
    fn assistant_style(&self) -> Style {
        Style::default().fg(Color::White)
    }
    fn thinking_style(&self) -> Style {
        Style::default().fg(Color::Magenta)
    }
    fn thinking_bold_style(&self) -> Style {
        Style::default()
            .fg(Color::Magenta)
            .add_modifier(Modifier::BOLD)
    }
    fn tool_call_style(&self) -> Style {
        Style::default().fg(Color::Cyan)
    }
    fn tool_call_bold_style(&self) -> Style {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    }
    fn success_style(&self) -> Style {
        Style::default().fg(Color::Green)
    }
    fn user_prefix(&self) -> &'static str {
        "\u{25b6} "
    }
    fn assistant_prefix(&self) -> &'static str {
        ""
    }
    fn thinking_prefix(&self) -> &'static str {
        "\u{1f4ad} "
    }
    fn tool_prefix(&self) -> &'static str {
        "\u{1f527} "
    }
    fn tool_ok_prefix(&self) -> &'static str {
        "  \u{2713}"
    }
    fn tool_err_prefix(&self) -> &'static str {
        "  \u{2717}"
    }
    fn done_prefix(&self) -> &'static str {
        "\u{2705} "
    }
    fn error_prefix(&self) -> &'static str {
        "\u{274c} "
    }
}
