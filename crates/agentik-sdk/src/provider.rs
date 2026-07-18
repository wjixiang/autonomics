//! Provider type presets.
//!
//! Each submodule exposes `preset_models()` — the catalogue of models a given
//! provider type offers, as metadata-only [`ModelInfo`](crate::model::ModelInfo)
//! entries (capabilities + pricing, `provider_id` left nil). Connection config
//! is *not* baked in here; it lives on a user-configured
//! [`ProviderConfig`](crate::model::ProviderConfig), which a model references
//! via `provider_id`. The server's `provider_registry` joins these presets with
//! a provider instance when the user creates one.

pub mod client;
pub mod deepseek;
pub mod mimo;
pub mod minimax;
pub mod registry;
pub mod sensenova;
pub mod zai;

use crate::model::{ModelInfo, ProviderType};

/// Implemented by each built-in provider module to expose its preset model
/// catalogue and default connection endpoint.
///
/// All methods are associated functions (no `&self`) — the trait formalises the
/// interface that every provider struct already follows ad-hoc.
pub trait ProviderPreset {
    /// The [`ProviderType`] variant this preset corresponds to.
    fn provider_type() -> ProviderType;

    /// Canonical preset models. All entries have `provider_id = Uuid::nil()`.
    /// The caller assigns a real `provider_id` when binding to a `ProviderConfig`.
    fn preset_models() -> Vec<ModelInfo>;

    /// Default base URL for this provider type, if one exists.
    /// Returns `""` for providers that require explicit configuration
    /// (e.g. minimax).
    fn default_base_url() -> &'static str;
}
