use std::collections::HashMap;
use std::sync::atomic::AtomicU32;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::model::{Model, ModelInfo, ProviderConfig};

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

/// Pool configuration — the normalized provider/model pair.
///
/// `providers` is the master list (connection config lives here, once per
/// endpoint). `models` reference a provider by `provider_id`. `ModelPool` joins
/// the two at build time, so the runtime hot path is unchanged.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelPoolConfig {
    pub providers: Vec<ProviderConfig>,
    pub models: Vec<ModelInfo>,
}

impl ModelPool {
    pub fn new() -> Self {
        Self {
            model_list: vec![],
            model_index: AtomicU32::new(0),
        }
    }

    /// Build a pool by joining each model with its referenced provider.
    ///
    /// A model whose `provider_id` matches no provider in `config.providers`
    /// yields a `BuildError` (rather than being silently dropped), since that
    /// indicates a dangling reference — typically a config/migration bug.
    pub fn from_config(config: ModelPoolConfig) -> Result<Self, ModelPoolError> {
        let mut pool = Self::new();

        let by_id: HashMap<Uuid, &ProviderConfig> =
            config.providers.iter().map(|p| (p.id, p)).collect();

        for model_info in config.models {
            let provider = by_id.get(&model_info.provider_id).ok_or_else(|| {
                ModelPoolError::BuildError(format!(
                    "model '{}' references unknown provider_id {}",
                    model_info.model_name, model_info.provider_id
                ))
            })?;
            let model =
                Model::new(model_info, provider).map_err(|e| ModelPoolError::BuildError(e.to_string()))?;
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
