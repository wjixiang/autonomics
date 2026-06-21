use std::sync::atomic::AtomicU32;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::model::{Model, ModelInfo};

#[derive(Default)]
pub struct ModelPool {
    model_list: Vec<Arc<Model>>,
    model_index: AtomicU32,
}

#[derive(Debug, Error)]
pub enum ModelPoolError {
    #[error("None model exist in ModelPool")]
    EmptyPool,
    #[error("Model not found: {0}")]
    ModelNotFound(String),
    #[error("Failed to build model: {0}")]
    BuildError(String),
}

/// Pool configuration — a flat list of fully-configured `ModelInfo` entries.
///
/// Each `ModelInfo` already carries its own connection config (`base_url`,
/// `api_key`, `auth_method`), so no separate provider layer is needed.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelPoolConfig {
    pub models: Vec<ModelInfo>,
}

impl ModelPool {
    pub fn new() -> Self {
        Self {
            model_list: vec![],
            model_index: AtomicU32::new(0),
        }
    }

    /// Build a pool from a list of fully-configured model infos.
    pub fn from_config(config: ModelPoolConfig) -> Result<Self, ModelPoolError> {
        let mut pool = Self::new();

        for model_info in config.models {
            let model = Model::new(model_info).map_err(|e| ModelPoolError::BuildError(e.to_string()))?;
            pool.add_model(model);
        }

        Ok(pool)
    }

    pub fn add_model(&mut self, model: Model) -> &mut Self {
        self.model_list.push(Arc::new(model));
        self
    }

    pub fn get_model_roundrobin(&self) -> Result<Arc<Model>, ModelPoolError> {
        if self.model_list.is_empty() {
            return Err(ModelPoolError::EmptyPool);
        }

        let count = self
            .model_index
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let index = count % self.model_list.len() as u32;

        self.model_list
            .get(index as usize)
            .cloned()
            .ok_or(ModelPoolError::EmptyPool)
    }

    pub fn get_model_by_name(&self, name: &str) -> Result<Arc<Model>, ModelPoolError> {
        self.model_list
            .iter()
            .find(|m| m.model_info.model_name == name)
            .cloned()
            .ok_or_else(|| ModelPoolError::ModelNotFound(name.to_string()))
    }

    pub fn model_names(&self) -> Vec<String> {
        self.model_list
            .iter()
            .map(|m| m.model_info.model_name.clone())
            .collect()
    }
}
