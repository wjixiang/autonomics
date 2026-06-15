//! Reusable settings panel for model configuration.
//!
//! Provides [`SettingsPanelState`] (editable providers + pool entries),
//! [`render_settings_panel`] (ratatui rendering), and keyboard handling
//! that produces [`SettingsAction`] values the host can act on.
//!
//! All types exchanged with the host come from `agentik_runtime` — the
//! settings panel never depends on `agentik_core` / `agentik_sdk` directly.

pub mod input;
pub mod renderer;
pub mod state;
pub mod theme;

use agentik_runtime::ModelConfig;

pub use state::{NewProviderForm, SettingsAction, SettingsPane, SettingsPanelState};
pub use theme::{DefaultSettingsTheme, SettingsTheme};
