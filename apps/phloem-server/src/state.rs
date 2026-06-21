//! Shared application state passed to all axum handlers via `State<AppState>`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use agentik_sdk::model::model_pool::ModelPool;
use tokio::sync::{RwLock, broadcast};
use uuid::Uuid;

use agentik_sdk::types::AgentEvent;

use crate::config::AppConfig;
use crate::services::agent_manager::AgentHandle;

/// Global application state shared across all request handlers.
pub struct AppState {
    /// Full application config (read/write via settings API).
    pub config: RwLock<AppConfig>,
    /// Path to the config file on disk.
    pub config_path: PathBuf,
    /// Runtime model pool. Wrapped in RwLock for hot-reload on settings save.
    pub model_pool: RwLock<Arc<ModelPool>>,
    /// Running agent instances, keyed by their UUID.
    pub agents: RwLock<HashMap<Uuid, AgentHandle>>,
    /// Global event bus — bridge tasks forward per-agent events here.
    pub event_broker: broadcast::Sender<(Uuid, AgentEvent)>,
}

impl AppState {
    pub fn new(config: AppConfig, config_path: PathBuf) -> Self {
        let model_pool = Arc::new(
            ModelPool::from_config(config.model_pool_config())
                .expect("failed to build model pool from config"),
        );

        let (event_broker, _) = broadcast::channel(1024);
        Self {
            config: RwLock::new(config),
            config_path,
            model_pool: RwLock::new(model_pool),
            agents: RwLock::new(HashMap::new()),
            event_broker,
        }
    }
}
