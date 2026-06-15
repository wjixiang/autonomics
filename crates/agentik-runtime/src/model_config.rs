//! Declarative model-configuration types for the frontend.
//!
//! These are pure-data, `serde`-serialisable structs with **no dependency on
//! `agentik-core`** or `agentik-sdk` internals.  The frontend constructs a
//! [`ModelConfig`] and hands it to [`ProcessManager::configure_pool`]
//! (or `reconfigure_pool`); the runtime translates it into a live
//! [`ModelPool`](agentik_sdk::model::model_pool::ModelPool) internally.

/// A single provider entry, persisted by the frontend / host.
///
/// Shape mirrors `dendrite/kms_tui/src/settings.rs` `ProviderConfig` but
/// lives in `agentik-runtime` so the frontend never imports SDK provider types.
#[derive(serde::Serialize, serde::Deserialize, Default, Clone, Debug, PartialEq, Eq)]
pub struct ProviderConfig {
    /// Stable unique id; referenced by [`PoolEntry::provider_id`].
    pub id: String,
    /// User-chosen display name (e.g. "mimo-prod").
    pub display_name: String,
    /// Which built-in provider type this instantiates
    /// (`"mimo"`, `"minimax"`, `"sensenova"`, `"deepseek"`, `"zai"`).
    pub provider_type: String,
    /// API key for this provider.
    pub api_key: String,
    /// Base URL.  Empty string means "use the SDK's built-in default".
    #[serde(default)]
    pub base_url: String,
    /// User-curated model list for this provider.  Empty = let the SDK pick.
    #[serde(default)]
    pub models: Vec<String>,
}

/// A single model entry in the pool.  References a [`ProviderConfig`] by its
/// stable [`ProviderConfig::id`].
#[derive(serde::Serialize, serde::Deserialize, Default, Clone, Debug, PartialEq, Eq)]
pub struct PoolEntry {
    pub provider_id: String,
    pub model: String,
}

/// Top-level model configuration passed by the frontend.
///
/// The runtime builds a shared [`ModelPool`](agentik_sdk::model::model_pool::ModelPool)
/// from this at [`configure_pool`](crate::ProcessManager::configure_pool) time.
#[derive(serde::Serialize, serde::Deserialize, Default, Clone, Debug)]
pub struct ModelConfig {
    #[serde(default)]
    pub providers: Vec<ProviderConfig>,
    #[serde(default)]
    pub pool: Vec<PoolEntry>,
}

impl ModelConfig {
    /// Deserialise from a JSON string.  Useful for hosts that read a config
    /// file; the runtime itself does **not** own any file path.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Serialise to a pretty JSON string.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}
