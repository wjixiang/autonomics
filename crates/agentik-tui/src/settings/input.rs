//! Keyboard handling for the settings panel.
//!
//! Accepts generic key parameters (`code`, `char`, `modifiers`) so the
//! host can translate from any terminal event source without the settings
//! panel depending on `crossterm` directly.

use agentik_runtime::provider_factory;

use super::state::{NewProviderForm, SettingsAction, SettingsPane, SettingsPanelState};

/// Key event abstraction.  Hosts translate from their terminal library
/// (crossterm, termion, etc.) into this.
#[derive(Debug, Clone)]
pub struct SettingsKey {
    pub code: SettingsKeyCode,
    pub modifiers: SettingsKeyModifiers,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsKeyCode {
    Char(char),
    Up,
    Down,
    Left,
    Right,
    Enter,
    Backspace,
    Tab,
    BackTab,
    Esc,
    Delete,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SettingsKeyModifiers {
    pub shift: bool,
}

impl SettingsKeyModifiers {
    pub fn shift(self) -> bool {
        self.shift
    }
}

/// Process a single key event against the settings panel state.
///
/// Returns `Some(SettingsAction)` when the user takes a finalising
/// action (apply / cancel).
pub fn handle_settings_key(
    state: &mut SettingsPanelState,
    key: SettingsKey,
) -> Option<SettingsAction> {
    // New-provider form captures all input while open.
    if state.new_provider_form.is_some() {
        return handle_form_key(state, key);
    }

    let result = match key.code {
        // ── Global ────────────────────────────────────
        SettingsKeyCode::Char('q') | SettingsKeyCode::Esc => {
            state.open = false;
            return Some(SettingsAction::Cancel);
        }
        SettingsKeyCode::Char('1') | SettingsKeyCode::Char('p') => {
            state.pane = SettingsPane::Providers;
            None
        }
        SettingsKeyCode::Char('2') | SettingsKeyCode::Char('o') => {
            state.pane = SettingsPane::Pool;
            None
        }
        SettingsKeyCode::Enter => {
            if !state.is_config_valid() {
                None
            } else {
                let config = state.configured_model_config();
                state.open = false;
                Some(SettingsAction::Apply(config))
            }
        }

        // ── Pane-specific ──────────────────────────────
        _ => match state.pane {
            SettingsPane::Providers => handle_providers_key(state, key),
            SettingsPane::Pool => handle_pool_key(state, key),
        },
    };

    result
}

fn handle_providers_key(state: &mut SettingsPanelState, key: SettingsKey) -> Option<SettingsAction> {
    match key.code {
        SettingsKeyCode::Up | SettingsKeyCode::Char('k') => {
            if state.selected_provider > 0 {
                state.selected_provider -= 1;
            }
        }
        SettingsKeyCode::Down | SettingsKeyCode::Char('j') => {
            if !state.providers.is_empty() {
                state.selected_provider = (state.selected_provider + 1).min(state.providers.len() - 1);
            }
        }
        SettingsKeyCode::Char('d') | SettingsKeyCode::Delete => {
            state.remove_selected_provider();
        }
        SettingsKeyCode::Char('n') => {
            state.new_provider_form = Some(NewProviderForm::default());
        }
        _ => {}
    }
    None
}

fn handle_pool_key(state: &mut SettingsPanelState, key: SettingsKey) -> Option<SettingsAction> {
    match key.code {
        SettingsKeyCode::Up | SettingsKeyCode::Char('k') => {
            if state.selected_pool > 0 {
                state.selected_pool -= 1;
            }
        }
        SettingsKeyCode::Down | SettingsKeyCode::Char('j') => {
            if !state.pool_entries.is_empty() {
                state.selected_pool = (state.selected_pool + 1).min(state.pool_entries.len() - 1);
            }
        }
        SettingsKeyCode::Char(' ') => {
            state.toggle_selected_pool_entry();
        }
        SettingsKeyCode::Char('d') | SettingsKeyCode::Delete => {
            state.remove_selected_pool_entry();
        }
        SettingsKeyCode::Char('a') => {
            // Quick-add: if there's exactly one provider, add a pool entry
            // with its default model.
            if let Some(provider) = state.providers.first() {
                let model = provider_factory::default_models_for_type(&provider.provider_type)
                    .first()
                    .cloned()
                    .unwrap_or_default();
                if !model.is_empty() {
                    state.add_pool_entry(provider.id.clone(), model);
                }
            }
        }
        _ => {}
    }
    None
}

fn handle_form_key(state: &mut SettingsPanelState, key: SettingsKey) -> Option<SettingsAction> {
    let form = match &mut state.new_provider_form {
        Some(f) => f,
        None => return None,
    };

    match key.code {
        SettingsKeyCode::Esc => {
            state.new_provider_form = None;
        }
        SettingsKeyCode::Enter => {
            if form.is_valid() {
                let config = form.to_provider_config();
                state.providers.push(config);
                state.new_provider_form = None;
                state.selected_provider = state.providers.len() - 1;
            }
        }
        SettingsKeyCode::Tab | SettingsKeyCode::BackTab => {
            let dir: i32 = if key.code == SettingsKeyCode::BackTab {
                -1
            } else {
                1
            };
            form.field_index = (form.field_index as i32 + dir).rem_euclid(4) as usize;
        }
        SettingsKeyCode::Up | SettingsKeyCode::Down => {
            let dir: i32 = if key.code == SettingsKeyCode::Up {
                -1
            } else {
                1
            };
            if form.field_index == 0 {
                form.cycle_provider_type(dir);
            }
        }
        SettingsKeyCode::Char(c) => {
            let fields = form.fields_mut();
            fields[form.field_index].push(c);
        }
        SettingsKeyCode::Backspace => {
            let fields = form.fields_mut();
            if form.field_index < fields.len() {
                fields[form.field_index].pop();
            }
        }
        _ => {}
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: SettingsKeyCode) -> SettingsKey {
        SettingsKey {
            code,
            modifiers: SettingsKeyModifiers::default(),
        }
    }

    fn char_key(c: char) -> SettingsKey {
        key(SettingsKeyCode::Char(c))
    }

    #[test]
    fn esc_cancels_and_closes() {
        let mut state = SettingsPanelState::new();
        state.open = true;
        let action = handle_settings_key(&mut state, key(SettingsKeyCode::Esc));
        assert!(matches!(action, Some(SettingsAction::Cancel)));
        assert!(!state.is_open());
    }

    #[test]
    fn enter_applies_when_valid() {
        let mut state = SettingsPanelState::new();
        state.open = true;
        state.providers.push(test_provider());
        state.add_pool_entry("p1".into(), "mimo-v2.5".into());

        let action = handle_settings_key(&mut state, key(SettingsKeyCode::Enter));
        assert!(matches!(action, Some(SettingsAction::Apply(_))));
        assert!(!state.is_open());
    }

    #[test]
    fn enter_does_nothing_when_invalid() {
        let mut state = SettingsPanelState::new();
        state.open = true;
        let action = handle_settings_key(&mut state, key(SettingsKeyCode::Enter));
        assert!(action.is_none());
        assert!(state.is_open());
    }

    #[test]
    fn add_provider_via_form() {
        let mut state = SettingsPanelState::new();
        state.open = true;

        // Open form
        handle_settings_key(&mut state, char_key('n'));
        assert!(state.new_provider_form.is_some());

        // Type api key
        let form = state.new_provider_form.as_mut().unwrap();
        form.field_index = 2; // key field
        handle_settings_key(&mut state, char_key('s'));
        handle_settings_key(&mut state, char_key('k'));
        handle_settings_key(&mut state, char_key('-'));
        handle_settings_key(&mut state, char_key('1'));

        // Submit
        handle_settings_key(&mut state, key(SettingsKeyCode::Enter));
        assert!(state.new_provider_form.is_none());
        assert_eq!(state.providers.len(), 1);
        assert_eq!(state.providers[0].api_key, "sk-1");
    }

    #[test]
    fn delete_removes_provider() {
        let mut state = SettingsPanelState::new();
        state.providers.push(test_provider());
        state.selected_provider = 0;

        handle_settings_key(&mut state, char_key('d'));
        assert!(state.providers.is_empty());
    }

    fn test_provider() -> agentik_runtime::ProviderConfig {
        agentik_runtime::ProviderConfig {
            id: "p1".into(),
            display_name: "test".into(),
            provider_type: "mimo".into(),
            api_key: "sk-test".into(),
            base_url: String::new(),
            models: Vec::new(),
        }
    }
}
