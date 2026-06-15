//! Theme surface for the Agent Status Panel.
//!
//! The renderer never touches a concrete theme type; it only knows
//! about the `AgentPanelTheme` trait. A host that already has a theme
//! struct just writes `impl AgentPanelTheme for MyTheme`. For
//! quick adoption a [`DefaultAgentPanelTheme`] is provided that
//! matches the colors and styles the panel had before extraction.

use ratatui::style::{Color, Modifier, Style};

/// Color and style getters the renderer depends on.
///
/// Keep this surface narrow: every method is one the renderer
/// genuinely reaches for. A host that doesn't need a custom value
/// can leave it as the [`DefaultAgentPanelTheme`] default by
/// forwarding to its own field.
pub trait AgentPanelTheme {
    // Foreground colors the renderer reaches for directly.
    fn text_primary(&self) -> Color;
    fn text_secondary(&self) -> Color;
    fn text_muted(&self) -> Color;
    fn spinner_color(&self) -> Color;
    fn tool_ok(&self) -> Color;
    fn tool_err(&self) -> Color;

    // Reusable styles. Returning `Style` keeps the renderer free of
    // any host-specific style type.
    fn error_style(&self) -> Style;
    fn success_style(&self) -> Style;
    fn tool_call_bold_style(&self) -> Style;
}

/// Sensible default colors and styles.
#[derive(Debug, Clone, Copy, Default)]
#[allow(dead_code)]
pub struct DefaultAgentPanelTheme;

impl AgentPanelTheme for DefaultAgentPanelTheme {
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
    fn error_style(&self) -> Style {
        Style::default().fg(Color::Red)
    }
    fn success_style(&self) -> Style {
        Style::default().fg(Color::Green)
    }
    fn tool_call_bold_style(&self) -> Style {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    }
}
