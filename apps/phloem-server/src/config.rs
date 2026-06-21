//! Application configuration — server settings + model pool config.
//!
//! Persisted as a single JSON file (`phloem.json`). Loaded at startup,
//! writable via `PUT /api/settings`.

use std::path::Path;

use agentik_sdk::model::model_pool::ModelPoolConfig;
use serde::{Deserialize, Serialize};

/// Top-level application configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub models: Vec<agentik_sdk::model::ModelInfo>,
}

/// Server network configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
}

fn default_host() -> String {
    "0.0.0.0".to_string()
}

fn default_port() -> u16 {
    3000
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            models: Vec::new(),
        }
    }
}

impl AppConfig {
    /// Load config from a JSON file. Returns default if file doesn't exist.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            tracing::warn!("config file {:?} not found, using defaults", path);
            let config = Self::default();
            config.save(path)?;
            return Ok(config);
        }

        let content = std::fs::read_to_string(path)?;
        let config: AppConfig = serde_json::from_str(&content)?;
        tracing::info!("loaded config from {:?} ({} models)", path, config.models.len());
        Ok(config)
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

    /// Extract the model pool config portion.
    pub fn model_pool_config(&self) -> ModelPoolConfig {
        ModelPoolConfig {
            models: self.models.clone(),
        }
    }

    /// Extract the server config portion.
    pub fn server_config(&self) -> ServerConfig {
        self.server.clone()
    }
}
