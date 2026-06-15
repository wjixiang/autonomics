//! Renders the settings panel as a sequence of `Line<'static>`.

use ratatui::text::{Line, Span};

use super::state::{SettingsPane, SettingsPanelState};
use super::theme::SettingsTheme;

/// Render the full settings panel.  Returns logical lines ready to be
/// wrapped in a `Paragraph` by the host.
pub fn render_settings_panel(state: &SettingsPanelState, theme: &dyn SettingsTheme) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    // Title
    lines.push(Line::from(Span::styled(
        "Settings".to_string(),
        theme.title_style(),
    )));

    // Pane tabs
    lines.push(Line::from(vec![
        Span::styled(
            "1 Providers".to_string(),
            theme.pane_tab_style(state.pane == SettingsPane::Providers),
        ),
        Span::styled("  ".to_string(), theme.normal_style()),
        Span::styled(
            "2 Pool".to_string(),
            theme.pane_tab_style(state.pane == SettingsPane::Pool),
        ),
    ]));

    // Empty line
    lines.push(Line::from(""));

    match state.pane {
        SettingsPane::Providers => render_providers(state, theme, &mut lines),
        SettingsPane::Pool => render_pool(state, theme, &mut lines),
    }

    // Key hints
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " [Tab] switch pane  [Enter] apply  [Esc] cancel  [n] new provider  [d] delete  [↑↓/jk] navigate",
        theme.key_hint_style(),
    )));

    lines
}

fn render_providers(
    state: &SettingsPanelState,
    theme: &dyn SettingsTheme,
    lines: &mut Vec<Line<'static>>,
) {
    if state.providers.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No providers configured. Press [n] to add one.",
            theme.muted_style(),
        )));
        return;
    }

    // Header
    lines.push(Line::from(Span::styled(
        "  Type           Name              API Key",
        theme.header_style(),
    )));

    for (i, prov) in state.providers.iter().enumerate() {
        let style = if i == state.selected_provider {
            theme.selected_style()
        } else {
            theme.normal_style()
        };

        let marker = if i == state.selected_provider { "▸" } else { " " };
        let masked_key = SettingsPanelState::mask_api_key(&prov.api_key);
        let line_text = format!(
            "{} {:<14} {:<18} {}",
            marker, prov.provider_type, prov.display_name, masked_key
        );
        lines.push(Line::from(Span::styled(line_text, style)));
    }
}

fn render_pool(state: &SettingsPanelState, theme: &dyn SettingsTheme, lines: &mut Vec<Line<'static>>) {
    if state.pool_entries.is_empty() && state.providers.is_empty() {
        lines.push(Line::from(Span::styled(
            "  Add providers first, then add pool entries with [a].",
            theme.muted_style(),
        )));
        return;
    }

    if state.pool_entries.is_empty() {
        lines.push(Line::from(Span::styled(
            "  Pool is empty. Press [a] to add the first entry.",
            theme.muted_style(),
        )));
        return;
    }

    // Header
    lines.push(Line::from(Span::styled(
        "  Status  Provider          Model",
        theme.header_style(),
    )));

    for (i, entry) in state.pool_entries.iter().enumerate() {
        let style = if i == state.selected_pool {
            theme.selected_style()
        } else {
            theme.normal_style()
        };
        let enabled = state.pool_enabled.get(i).copied().unwrap_or(false);

        let indicator = if enabled {
            theme.enabled_indicator()
        } else {
            theme.disabled_indicator()
        };
        let provider_name = state.provider_display_name(&entry.provider_id);
        let marker = if i == state.selected_pool { "▸" } else { " " };

        let line_text = format!("{} {} {:<17} {}", marker, indicator, provider_name, entry.model);
        lines.push(Line::from(Span::styled(line_text, style)));
    }

    // Also render the new-provider form if open.
    if let Some(form) = &state.new_provider_form {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("  New Provider", theme.header_style())));

        let fields = [
            ("Type", &form.provider_type, form.field_index == 0),
            ("Name", &form.display_name, form.field_index == 1),
            ("Key", &form.api_key, form.field_index == 2),
            ("URL", &form.base_url, form.field_index == 3),
        ];
        for (label, value, focused) in &fields {
            let label_span = Span::styled(format!("  {:>5}: ", label), theme.form_label_style());
            let value_style = if *focused {
                theme.form_cursor_style()
            } else {
                theme.form_value_style()
            };
            let display_value = if *label == "Key" && !value.is_empty() {
                SettingsPanelState::mask_api_key(value.as_str())
            } else {
                value.to_string()
            };
            lines.push(Line::from(vec![
                label_span,
                Span::styled(display_value, value_style),
                if *focused {
                    Span::styled("█".to_string(), theme.form_cursor_style())
                } else {
                    Span::raw("")
                },
            ]));
        }
        lines.push(Line::from(Span::styled(
            "  [Tab] next field  [↑/↓] cycle type  [Enter] save  [Esc] cancel",
            theme.key_hint_style(),
        )));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::theme::DefaultSettingsTheme;

    fn theme() -> DefaultSettingsTheme {
        DefaultSettingsTheme
    }

    #[test]
    fn empty_panel_renders_hint() {
        let state = SettingsPanelState::new();
        let lines = render_settings_panel(&state, &theme());
        let joined = lines.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n");
        assert!(joined.contains("No providers configured"));
    }

    #[test]
    fn providers_render_selected() {
        let mut state = SettingsPanelState::new();
        state.providers.push(make_test_provider("mimo", "mimo-test"));
        let lines = render_settings_panel(&state, &theme());
        let joined = lines.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n");
        assert!(joined.contains("mimo"));
        assert!(joined.contains("mimo-test"));
    }

    #[test]
    fn pool_renders_toggle_status() {
        let mut state = SettingsPanelState::new();
        state.providers.push(make_test_provider("mimo", "m1"));
        state.add_pool_entry("p1".into(), "mimo-v2.5".into());
        let lines = render_settings_panel(&state, &theme());
        let joined = lines.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n");
        // Default enabled indicator (◉)
        assert!(joined.contains("◉"));
    }

    fn make_test_provider(ptype: &str, name: &str) -> agentik_runtime::ProviderConfig {
        agentik_runtime::ProviderConfig {
            id: "p1".to_string(),
            display_name: name.to_string(),
            provider_type: ptype.to_string(),
            api_key: "sk-test-key-12345678".to_string(),
            base_url: String::new(),
            models: Vec::new(),
        }
    }
}
