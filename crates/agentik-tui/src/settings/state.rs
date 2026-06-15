//! Settings panel state: editable providers + pool entries.
//!
//! The panel holds in-memory copies of providers and pool entries.
//! When the host calls [`configured_model_config`](Self::configured_model_config),
//! it gets a [`ModelConfig`] ready to pass to
//! [`ProcessManager::configure_pool`](agentik_runtime::ProcessManager::configure_pool).

use agentik_runtime::{ModelConfig, PoolEntry, ProviderConfig};
use agentik_runtime::provider_factory;

// ── Actions ─────────────────────────────────────────────────

/// Actions produced by the settings panel for the host to handle.
#[derive(Debug, Clone)]
pub enum SettingsAction {
    /// User confirmed the settings.  Contains the full model config.
    Apply(ModelConfig),

    /// User cancelled / closed the settings panel.
    Cancel,
}

// ── Pane selection ──────────────────────────────────────────

/// Which sub-pane is active inside the settings modal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsPane {
    Providers,
    Pool,
}

impl Default for SettingsPane {
    fn default() -> Self {
        Self::Providers
    }
}

// ── New-provider form ───────────────────────────────────────

/// Inline form for adding a new provider.
#[derive(Debug, Clone)]
pub struct NewProviderForm {
    pub provider_type: String,
    pub display_name: String,
    pub api_key: String,
    pub base_url: String,
    /// Which field the cursor is in (0 = type, 1 = name, 2 = key, 3 = url).
    pub field_index: usize,
}

impl Default for NewProviderForm {
    fn default() -> Self {
        Self {
            provider_type: provider_factory::builtin_provider_types()
                .first()
                .map(|s| s.to_string())
                .unwrap_or_default(),
            display_name: String::new(),
            api_key: String::new(),
            base_url: String::new(),
            field_index: 0,
        }
    }
}

impl NewProviderForm {
    /// Cycle through built-in provider types.
    pub fn cycle_provider_type(&mut self, direction: i32) {
        let types = provider_factory::builtin_provider_types();
        if types.is_empty() {
            return;
        }
        let current = types.iter().position(|t| *t == self.provider_type);
        let len = types.len() as i32;
        let next = match current {
            Some(idx) => ((idx as i32 + direction).rem_euclid(len)) as usize,
            None => 0,
        };
        self.provider_type = types[next].to_string();
    }

    /// Fields as a mutable slice for cursor editing.
    pub fn fields_mut(&mut self) -> [&mut String; 4] {
        [
            &mut self.provider_type,
            &mut self.display_name,
            &mut self.api_key,
            &mut self.base_url,
        ]
    }

    /// Whether the form has enough data to create a provider.
    pub fn is_valid(&self) -> bool {
        !self.api_key.is_empty()
    }

    /// Convert the form into a [`ProviderConfig`], generating a unique id.
    pub fn to_provider_config(&self) -> ProviderConfig {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        ProviderConfig {
            id: format!("prov-{:x}", nanos),
            display_name: if self.display_name.is_empty() {
                self.provider_type.clone()
            } else {
                self.display_name.clone()
            },
            provider_type: self.provider_type.clone(),
            api_key: self.api_key.clone(),
            base_url: self.base_url.clone(),
            models: Vec::new(),
        }
    }
}

// ── Settings panel state ─────────────────────────────────────

/// Editable model-configuration state for the settings panel.
#[derive(Debug, Clone)]
pub struct SettingsPanelState {
    /// Which sub-pane is active.
    pub pane: SettingsPane,
    /// Editable provider list.
    pub providers: Vec<ProviderConfig>,
    /// Editable pool entries.
    pub pool_entries: Vec<PoolEntry>,
    /// Whether each pool entry is enabled.
    pub pool_enabled: Vec<bool>,
    /// Cursor index in the providers list.
    pub selected_provider: usize,
    /// Cursor index in the pool list.
    pub selected_pool: usize,
    /// Whether the new-provider form is open.
    pub new_provider_form: Option<NewProviderForm>,
    /// Whether the settings modal is open.
    pub open: bool,
}

impl Default for SettingsPanelState {
    fn default() -> Self {
        Self::new()
    }
}

impl SettingsPanelState {
    /// Create a new, empty settings panel.
    pub fn new() -> Self {
        Self {
            pane: SettingsPane::Providers,
            providers: Vec::new(),
            pool_entries: Vec::new(),
            pool_enabled: Vec::new(),
            selected_provider: 0,
            selected_pool: 0,
            new_provider_form: None,
            open: false,
        }
    }

    /// Create a settings panel pre-loaded from an existing [`ModelConfig`].
    pub fn from_config(config: &ModelConfig) -> Self {
        let pool_enabled = vec![true; config.pool.len()];
        Self {
            providers: config.providers.clone(),
            pool_entries: config.pool.clone(),
            pool_enabled,
            ..Self::new()
        }
    }

    /// Toggle the settings modal open/closed.
    pub fn toggle_open(&mut self) {
        self.open = !self.open;
    }

    /// Whether the settings modal is currently open.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Clamp cursor indices to valid ranges.
    fn clamp_cursors(&mut self) {
        if self.providers.is_empty() {
            self.selected_provider = 0;
        } else {
            self.selected_provider = self.selected_provider.min(self.providers.len() - 1);
        }
        if self.pool_entries.is_empty() {
            self.selected_pool = 0;
        } else {
            self.selected_pool = self.selected_pool.min(self.pool_entries.len() - 1);
        }
    }

    /// Build a [`ModelConfig`] from the currently enabled pool entries.
    /// Only entries with `pool_enabled[i] == true` are included.
    pub fn configured_model_config(&self) -> ModelConfig {
        let pool: Vec<PoolEntry> = self
            .pool_entries
            .iter()
            .zip(self.pool_enabled.iter())
            .filter(|(_, enabled)| **enabled)
            .map(|(entry, _)| entry.clone())
            .collect();
        ModelConfig {
            providers: self.providers.clone(),
            pool,
        }
    }

    /// Whether the current config is valid (has at least one provider and
    /// one enabled pool entry).
    pub fn is_config_valid(&self) -> bool {
        !self.providers.is_empty()
            && self
                .pool_enabled
                .iter()
                .any(|&e| e)
    }

    /// Whether the pool is empty (no entries at all).
    pub fn is_pool_empty(&self) -> bool {
        self.pool_entries.is_empty()
    }

    /// Whether providers are empty.
    pub fn is_providers_empty(&self) -> bool {
        self.providers.is_empty()
    }

    /// Get provider types available for new providers.
    pub fn builtin_provider_types(&self) -> &'static [&'static str] {
        provider_factory::builtin_provider_types()
    }

    // ── Provider operations ─────────────────────────────────

    /// Remove the currently selected provider and any pool entries
    /// referencing it.
    pub fn remove_selected_provider(&mut self) -> Option<ProviderConfig> {
        if self.providers.is_empty() {
            return None;
        }
        let removed = self.providers.remove(self.selected_provider);
        // Remove pool entries referencing the removed provider.
        let removed_id = removed.id.clone();
        let mut i = 0;
        while i < self.pool_entries.len() {
            if self.pool_entries[i].provider_id == removed_id {
                self.pool_entries.remove(i);
                self.pool_enabled.remove(i);
            } else {
                i += 1;
            }
        }
        self.clamp_cursors();
        Some(removed)
    }

    // ── Pool operations ──────────────────────────────────────

    /// Remove the currently selected pool entry.
    pub fn remove_selected_pool_entry(&mut self) -> Option<PoolEntry> {
        if self.pool_entries.is_empty() {
            return None;
        }
        let removed = self.pool_entries.remove(self.selected_pool);
        self.pool_enabled.remove(self.selected_pool);
        self.clamp_cursors();
        Some(removed)
    }

    /// Toggle the currently selected pool entry on/off.
    pub fn toggle_selected_pool_entry(&mut self) {
        if let Some(enabled) = self.pool_enabled.get_mut(self.selected_pool) {
            *enabled = !*enabled;
        }
    }

    /// Add a pool entry for a given provider.
    pub fn add_pool_entry(&mut self, provider_id: String, model: String) {
        self.pool_entries.push(PoolEntry {
            provider_id,
            model,
        });
        self.pool_enabled.push(true);
        self.selected_pool = self.pool_entries.len() - 1;
    }

    /// Get the display name for a provider by id.
    pub fn provider_display_name(&self, id: &str) -> String {
        self.providers
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.display_name.clone())
            .unwrap_or_else(|| id.to_string())
    }

    /// Mask an API key for display (show first 4 and last 4 chars).
    pub fn mask_api_key(key: &str) -> String {
        let bytes = key.as_bytes();
        if bytes.len() <= 8 {
            return "••••••••".to_string();
        }
        let mut s = String::with_capacity(bytes.len() + 4);
        s.push_str(std::str::from_utf8(&bytes[..4]).unwrap_or(""));
        s.push_str("••");
        s.push_str(std::str::from_utf8(&bytes[..4]).unwrap_or(""));
        s.push_str("••");
        s.push_str(std::str::from_utf8(&bytes[bytes.len() - 4..]).unwrap_or(""));
        s
    }
}

// ── Tests ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_provider(id: &str, ptype: &str) -> ProviderConfig {
        ProviderConfig {
            id: id.to_string(),
            display_name: format!("{ptype}-test"),
            provider_type: ptype.to_string(),
            api_key: "sk-test-key-12345678".to_string(),
            base_url: String::new(),
            models: Vec::new(),
        }
    }

    #[test]
    fn from_config_preserves_entries() {
        let mut cfg = ModelConfig::default();
        cfg.providers.push(make_provider("p1", "mimo"));
        cfg.pool.push(PoolEntry {
            provider_id: "p1".into(),
            model: "mimo-v2.5".into(),
        });

        let state = SettingsPanelState::from_config(&cfg);
        assert_eq!(state.providers.len(), 1);
        assert_eq!(state.pool_entries.len(), 1);
        assert!(state.pool_enabled[0]);
    }

    #[test]
    fn configured_model_config_filters_disabled() {
        let mut state = SettingsPanelState::new();
        state.providers.push(make_provider("p1", "mimo"));
        state.providers.push(make_provider("p2", "minimax"));
        state.add_pool_entry("p1".into(), "mimo-v2.5".into());
        state.add_pool_entry("p2".into(), "MiniMax-M2.7".into());
        state.pool_enabled[1] = false;

        let config = state.configured_model_config();
        assert_eq!(config.pool.len(), 1);
        assert_eq!(config.pool[0].model, "mimo-v2.5");
    }

    #[test]
    fn remove_provider_cascades_to_pool() {
        let mut state = SettingsPanelState::new();
        state.providers.push(make_provider("p1", "mimo"));
        state.providers.push(make_provider("p2", "minimax"));
        state.add_pool_entry("p1".into(), "mimo-v2.5".into());
        state.add_pool_entry("p2".into(), "MiniMax-M2.7".into());

        state.selected_provider = 0;
        state.remove_selected_provider();
        assert_eq!(state.providers.len(), 1);
        assert_eq!(state.pool_entries.len(), 1);
        assert_eq!(state.pool_entries[0].provider_id, "p2");
    }

    #[test]
    fn mask_api_key_short() {
        assert_eq!(SettingsPanelState::mask_api_key("short"), "••••••••");
    }

    #[test]
    fn mask_api_key_long() {
        let masked = SettingsPanelState::mask_api_key("sk-abcdefghijklmnopqrstuv");
        assert!(masked.starts_with("sk-a"));
        assert!(masked.ends_with("tuv"));
        assert!(masked.contains('•'));
    }

    #[test]
    fn new_provider_form_cycle() {
        let mut form = NewProviderForm::default();
        let types = provider_factory::builtin_provider_types();
        assert_eq!(form.provider_type, types[0]);
        form.cycle_provider_type(1);
        assert_eq!(form.provider_type, types[1]);
        form.cycle_provider_type(1);
        // wrap around
        assert_eq!(form.provider_type, types[0]);
    }

    #[test]
    fn new_provider_form_to_config() {
        let mut form = NewProviderForm::default();
        form.api_key = "sk-test".into();
        let config = form.to_provider_config();
        assert!(!config.id.is_empty());
        assert_eq!(config.provider_type, "mimo");
        assert_eq!(config.api_key, "sk-test");
    }
}
