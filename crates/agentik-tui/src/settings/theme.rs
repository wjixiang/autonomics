//! Settings panel theme trait and default implementation.

use ratatui::style::{Color, Modifier, Style};

/// Theme for the settings panel.  Hosts can implement this to customise
/// colours and symbols.
pub trait SettingsTheme {
    fn title_style(&self) -> Style;
    fn pane_tab_style(&self, active: bool) -> Style;
    fn header_style(&self) -> Style;
    fn selected_style(&self) -> Style;
    fn normal_style(&self) -> Style;
    fn muted_style(&self) -> Style;
    fn enabled_indicator(&self) -> char;
    fn disabled_indicator(&self) -> char;
    fn key_hint_style(&self) -> Style;
    fn error_style(&self) -> Style;
    fn form_label_style(&self) -> Style;
    fn form_value_style(&self) -> Style;
    fn form_cursor_style(&self) -> Style;
}

/// Default colour scheme.
pub struct DefaultSettingsTheme;

impl SettingsTheme for DefaultSettingsTheme {
    fn title_style(&self) -> Style {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    }

    fn pane_tab_style(&self, active: bool) -> Style {
        if active {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        }
    }

    fn header_style(&self) -> Style {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    }

    fn selected_style(&self) -> Style {
        Style::default().fg(Color::White).bg(Color::DarkGray)
    }

    fn normal_style(&self) -> Style {
        Style::default().fg(Color::Gray)
    }

    fn muted_style(&self) -> Style {
        Style::default().fg(Color::DarkGray)
    }

    fn enabled_indicator(&self) -> char {
        '◉'
    }

    fn disabled_indicator(&self) -> char {
        '○'
    }

    fn key_hint_style(&self) -> Style {
        Style::default().fg(Color::DarkGray)
    }

    fn error_style(&self) -> Style {
        Style::default().fg(Color::Red)
    }

    fn form_label_style(&self) -> Style {
        Style::default().fg(Color::Cyan)
    }

    fn form_value_style(&self) -> Style {
        Style::default().fg(Color::White)
    }

    fn form_cursor_style(&self) -> Style {
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::UNDERLINED)
    }
}
