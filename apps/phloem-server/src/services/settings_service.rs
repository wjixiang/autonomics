//! Settings persistence service — read/write AppConfig to JSON file.

use std::sync::Arc;

use crate::config::AppConfig;
use crate::state::AppState;

/// Read the current application config.
pub async fn load_settings(state: &AppState) -> AppConfig {
    state.config.read().await.clone()
}

/// Write a new application config to disk and update in-memory state.
pub async fn save_settings(state: &Arc<AppState>, new_config: AppConfig) -> anyhow::Result<()> {
    // 1. Persist to disk first (fail fast if write fails)
    new_config.save(&state.config_path)?;

    // 2. Rebuild model pool from new config
    let new_pool = Arc::new(
        ModelPool::from_config(new_config.model_pool_config())
            .map_err(|e| anyhow::anyhow!("failed to build model pool: {e}"))?,
    );

    // 3. Update in-memory state
    *state.config.write().await = new_config;
    *state.model_pool.write().await = new_pool;

    tracing::info!("settings saved and model pool rebuilt");
    Ok(())
}

use agentik_sdk::model::model_pool::ModelPool;
