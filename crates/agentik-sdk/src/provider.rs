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
pub mod sensenova;
pub mod zai;
