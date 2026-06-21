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
    /// Build a model by name, using the provided API key for authentication.
    fn get_model(&self, model_name: &str, api_key: String) -> Result<Model, ProviderError>;

    fn add_models(&mut self, model: Vec<ModelInfo>);

    /// List available models (may query remote API or return presets).
    async fn list_models(&self, api_key: String) -> Result<Vec<Model>, ProviderError>;
}
