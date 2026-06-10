use thiserror::Error;

use crate::model::Model;
use std::sync::{Arc, atomic::AtomicU32};

#[derive(Default)]
pub struct ModelPool {
    model_list: Vec<Arc<Model>>,
    model_index: AtomicU32,
}

#[derive(Debug, Error)]
pub enum ModelPoolError {
    #[error("None LlmProvider exist in ModelPool")]
    EmptyPool,
    #[error("Model not found: {0}")]
    ModelNotFound(String),
}

impl ModelPool {
    pub fn new() -> Self {
        Self {
            model_list: vec![],
            model_index: AtomicU32::new(0),
        }
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

        let rr_model = self
            .model_list
            .get(index as usize)
            .ok_or(ModelPoolError::EmptyPool)?
            .clone();

        Ok(rr_model)
    }

    pub fn get_model_by_name(&self, name: &str) -> Result<Arc<Model>, ModelPoolError> {
        self.model_list
            .iter()
            .find(|m| m.model_info.model_name == name)
            .cloned()
            .ok_or_else(|| ModelPoolError::ModelNotFound(name.to_string()))
    }

    pub fn model_names(&self) -> Vec<String> {
        self.model_list.iter().map(|m| m.model_info.model_name.clone()).collect()
    }
}
