//! Static registry that maps [`ProviderType`] → preset models and default URLs.
//!
//! All lookup is compile-time dispatch via `match` — no runtime registration.

use crate::model::{ModelInfo, ProviderType};
use crate::provider::{
    ProviderPreset, deepseek::DeepseekProvider, mimo::MimoProvider, minimax::MinimaxProvider,
    sensenova::SensenovaProvider, zai::ZaiProvider,
};

/// Returns preset models for a known [`ProviderType`], or `None` for
/// [`ProviderType::Custom(_)`](ProviderType::Custom) (which has no baked-in
/// catalogue).
pub fn preset_models(provider_type: &ProviderType) -> Option<Vec<ModelInfo>> {
    match provider_type {
        ProviderType::Deepseek => Some(DeepseekProvider::preset_models()),
        ProviderType::Mimo => Some(MimoProvider::preset_models()),
        ProviderType::Minimax => Some(MinimaxProvider::preset_models()),
        ProviderType::Sensenova => Some(SensenovaProvider::preset_models()),
        ProviderType::Zai => Some(ZaiProvider::preset_models()),
        ProviderType::Custom(_) => None,
    }
}

/// Returns the default base URL for a known provider type, or `None` for
/// [`ProviderType::Custom(_)`](ProviderType::Custom).
pub fn default_base_url(provider_type: &ProviderType) -> Option<&'static str> {
    match provider_type {
        ProviderType::Deepseek => Some(DeepseekProvider::default_base_url()),
        ProviderType::Mimo => Some(MimoProvider::default_base_url()),
        ProviderType::Minimax => Some(MinimaxProvider::default_base_url()),
        ProviderType::Sensenova => Some(SensenovaProvider::default_base_url()),
        ProviderType::Zai => Some(ZaiProvider::default_base_url()),
        ProviderType::Custom(_) => None,
    }
}

/// Lists all built-in provider type names that have presets.
pub fn known_provider_types() -> Vec<&'static str> {
    vec!["deepseek", "mimo", "minimax", "sensenova", "zai"]
}
