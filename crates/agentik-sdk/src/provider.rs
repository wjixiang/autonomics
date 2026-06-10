pub mod client;
pub mod deepseek;
pub mod mimo;
pub mod minimax;
pub mod sensenova;
pub mod zai;

use async_trait::async_trait;
use mockall::automock;

use crate::model::{Model, ModelInfo};
use agentik_types::errors::AnthropicError;

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("model '{0}' not found")]
    ModelNotFound(ModelInfo),

    #[error("client creation error: {0}")]
    ClientCreationError(#[from] AnthropicError),
}

#[automock]
#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn add_models(&mut self, model: Vec<ModelInfo>);
    fn get_model(&self, model_name: &str) -> Result<Model, ProviderError>;
    async fn list_models(&self) -> Result<Vec<Model>, ProviderError>;
}

#[derive(Clone)]
pub struct ProviderInfo {
    pub base_url: String,
    pub api_key: String,
    pub preset_models: Vec<ModelInfo>,
}
