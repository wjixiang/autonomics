//! Settings file I/O for the host binary.
//!
//! Reads / writes `data/settings.json` in the format defined by
//! [`agentik_runtime::ModelConfig`].

use std::fs;
use std::path::Path;

use agentik_runtime::ModelConfig;

/// Load settings from disk.  Returns a default (empty) config if the
/// file doesn't exist or can't be parsed.
pub fn load_settings(path: &str) -> ModelConfig {
    let data = match fs::read_to_string(path) {
        Ok(d) => d,
        Err(_) => {
            tracing::info!("No settings file at {path}, using defaults");
            return ModelConfig::default();
        }
    };
    match ModelConfig::from_json(&data) {
        Ok(config) => {
            tracing::info!("Loaded {} providers, {} pool entries from {}",
                config.providers.len(), config.pool.len(), path);
            config
        }
        Err(e) => {
            tracing::warn!("Failed to parse settings file: {e}; using defaults");
            ModelConfig::default()
        }
    }
}

/// Persist the current model config to disk.  Called after every
/// settings mutation so the file is always up-to-date.
pub fn save_settings(path: &str, config: &ModelConfig) {
    // Ensure parent directory exists.
    if let Some(parent) = Path::new(path).parent() {
        let _ = fs::create_dir_all(parent);
    }
    match config.to_json() {
        Ok(json) => {
            if let Err(e) = fs::write(path, json) {
                tracing::warn!("Failed to write settings: {e}");
            }
        }
        Err(e) => {
            tracing::warn!("Failed to serialize settings: {e}");
        }
    }
}
