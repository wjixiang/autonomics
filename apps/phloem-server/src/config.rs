//! Application configuration — server settings + provider/model configuration.
//!
//! Persisted as a single JSON file (`phloem.json`). Loaded at startup,
//! writable via `PUT /api/settings`.
//!
//! Providers are the "master" side: each holds connection config (base URL,
//! API key, auth) for one endpoint. Models reference a provider by id. Legacy
//! config files (where connection config was embedded in each model) are
//! migrated on load — see [`AppConfig::load`].

use std::path::Path;

use agentik_sdk::http::auth::AuthMethod;
use agentik_sdk::model::model_pool::ModelPoolConfig;
use agentik_sdk::model::{ModelInfo, ProviderConfig};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Top-level application configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub server: ServerConfig,
    /// Provider instances (master). Connection config lives here.
    #[serde(default)]
    pub providers: Vec<ProviderConfig>,
    /// Models (reference a provider via `provider_id`).
    #[serde(default)]
    pub models: Vec<ModelInfo>,
    /// UUID of the default agent. Stable across restarts — persisted here
    /// so memory can be restored on boot.
    #[serde(default)]
    pub default_agent_id: Option<Uuid>,
    /// Storage configuration for agent persistence.
    #[serde(default)]
    pub storage: StorageConfig,
}

/// Server network configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
}

/// Agent persistence storage configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    /// Path to the SQLite database for agent snapshots.
    #[serde(default = "default_agent_db_path")]
    pub agent_db_path: String,
}

fn default_host() -> String {
    "0.0.0.0".to_string()
}

fn default_port() -> u16 {
    3000
}

fn default_agent_db_path() -> String {
    "./data/agents.db".to_string()
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
        }
    }
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            agent_db_path: default_agent_db_path(),
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            providers: Vec::new(),
            models: Vec::new(),
            default_agent_id: None,
            storage: StorageConfig::default(),
        }
    }
}

// ── Legacy migration ──────────────────────────────────────────────────────

/// Shape of a model entry before the provider/model split — connection config
/// was embedded directly. Used only for one-shot migration on load.
#[derive(Debug, Deserialize)]
struct LegacyModel {
    model_name: String,
    provider_name: String,
    #[serde(default)]
    context_length: u64,
    #[serde(default)]
    max_output_tokens: u64,
    #[serde(default)]
    base_url: String,
    #[serde(default)]
    api_key: String,
    #[serde(default)]
    auth_method: AuthMethod,
}

/// Key that identifies a unique provider instance derived from legacy models.
/// Models sharing all four values are grouped under one provider. `AuthMethod`
/// lacks `Hash`/`Eq`, so its string discriminant is used.
#[derive(Debug, Hash, Eq, PartialEq, Clone)]
struct ProviderGroup {
    provider_type: String,
    base_url: String,
    api_key: String,
    auth_method: String,
}

fn auth_method_key(m: &AuthMethod) -> String {
    match m {
        AuthMethod::Anthropic => "Anthropic".to_string(),
        AuthMethod::Bearer => "Bearer".to_string(),
    }
}

/// Migrate a legacy config (connection config embedded in models) to the new
/// provider/model split. Models sharing the same `(type, base_url, api_key,
/// auth_method)` are grouped into a single provider instance.
fn migrate_legacy(
    server: ServerConfig,
    legacy_models: Vec<LegacyModel>,
    default_agent_id: Option<Uuid>,
    storage: StorageConfig,
) -> AppConfig {
    use std::collections::HashMap;

    let mut groups: HashMap<ProviderGroup, Uuid> = HashMap::new();
    let mut providers: Vec<ProviderConfig> = Vec::new();
    let mut models: Vec<ModelInfo> = Vec::new();

    for lm in legacy_models {
        let key = ProviderGroup {
            provider_type: lm.provider_name.clone(),
            base_url: lm.base_url.clone(),
            api_key: lm.api_key.clone(),
            auth_method: auth_method_key(&lm.auth_method),
        };
        let provider_id = *groups.entry(key).or_insert_with(|| {
            let provider = ProviderConfig {
                id: Uuid::new_v4(),
                name: format!(
                    "{}-{}",
                    lm.provider_name,
                    &lm.base_url[..lm.base_url.len().min(12)]
                ),
                provider_type: lm.provider_name.clone(),
                base_url: lm.base_url.clone(),
                api_key: lm.api_key.clone(),
                auth_method: lm.auth_method.clone(),
            };
            let id = provider.id;
            providers.push(provider);
            id
        });

        models.push(ModelInfo {
            model_name: lm.model_name,
            provider_id,
            context_length: lm.context_length,
            max_output_tokens: lm.max_output_tokens,
            vision_ability: false,
            supports_function_calling: true,
            supports_streaming: true,
            supports_thinking: false,
            input_token_price: 0.0,
            output_token_price: 0.0,
        });
    }

    AppConfig {
        server,
        providers,
        models,
        default_agent_id,
        storage,
    }
}

impl AppConfig {
    /// Load config from a JSON file. Returns default if file doesn't exist.
    ///
    /// Performs one-shot migration from the legacy schema (connection config
    /// embedded per-model) to the provider/model split, re-saving the file
    /// in the new format when migration occurs.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            tracing::warn!("config file {:?} not found, using defaults", path);
            let config = Self::default();
            config.save(path)?;
            return Ok(config);
        }

        let content = std::fs::read_to_string(path)?;
        let raw: serde_json::Value = serde_json::from_str(&content)?;

        // New format: try to deserialize directly. Bails out if the file is
        // legacy (models carry connection fields, no `providers`).
        if let Ok(config) = serde_json::from_value::<AppConfig>(raw.clone()) {
            tracing::info!(
                "loaded config from {:?} ({} providers, {} models)",
                path,
                config.providers.len(),
                config.models.len()
            );
            return Ok(config);
        }

        // Legacy path: pull out the shared fields, migrate models.
        tracing::info!("config {:?} is legacy format — migrating", path);
        let server = raw
            .get("server")
            .map(|v| serde_json::from_value(v.clone()))
            .transpose()?
            .unwrap_or_default();
        let storage = raw
            .get("storage")
            .map(|v| serde_json::from_value(v.clone()))
            .transpose()?
            .unwrap_or_default();
        let default_agent_id = raw
            .get("default_agent_id")
            .and_then(|v| serde_json::from_value(v.clone()).ok());
        let legacy_models: Vec<LegacyModel> = raw
            .get("models")
            .map(|v| serde_json::from_value(v.clone()))
            .transpose()?
            .unwrap_or_default();

        let migrated = migrate_legacy(server, legacy_models, default_agent_id, storage);
        migrated.save(path)?;
        tracing::info!(
            "migration complete: {} providers, {} models",
            migrated.providers.len(),
            migrated.models.len()
        );
        Ok(migrated)
    }

    /// Save config to a JSON file (creates parent dirs if needed).
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        tracing::info!("saved config to {:?}", path);
        Ok(())
    }

    /// Extract the model pool config portion (providers + models).
    pub fn model_pool_config(&self) -> ModelPoolConfig {
        ModelPoolConfig {
            providers: self.providers.clone(),
            models: self.models.clone(),
        }
    }

    /// Extract the server config portion.
    pub fn server_config(&self) -> ServerConfig {
        self.server.clone()
    }
}
