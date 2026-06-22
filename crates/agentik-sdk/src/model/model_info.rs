use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Metadata describing a single model entry in the pool.
///
/// `ModelInfo` holds only model capabilities — it does **not** carry connection
/// config. Connection details (`base_url`, `api_key`, `auth_method`) live on the
/// referenced [`crate::model::ProviderConfig`], looked up at pool-build time via
/// `provider_id`.
#[derive(Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub model_name: String,
    /// Reference to the provider instance that supplies this model's connection.
    pub provider_id: Uuid,
    pub context_length: u64,
    pub max_output_tokens: u64,
    pub vision_ability: bool,
    pub supports_function_calling: bool,
    pub supports_streaming: bool,
    pub supports_thinking: bool,
    pub input_token_price: f64,
    pub output_token_price: f64,
}

impl Default for ModelInfo {
    fn default() -> Self {
        Self {
            model_name: String::new(),
            provider_id: Uuid::nil(),
            context_length: 0,
            max_output_tokens: 0,
            vision_ability: false,
            supports_function_calling: false,
            supports_streaming: false,
            supports_thinking: false,
            input_token_price: 0.0,
            output_token_price: 0.0,
        }
    }
}

impl std::fmt::Debug for ModelInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModelInfo")
            .field("model_name", &self.model_name)
            .field("provider_id", &self.provider_id)
            .field("context_length", &self.context_length)
            .field("max_output_tokens", &self.max_output_tokens)
            .field("vision_ability", &self.vision_ability)
            .field("supports_function_calling", &self.supports_function_calling)
            .field("supports_streaming", &self.supports_streaming)
            .field("supports_thinking", &self.supports_thinking)
            .field("input_token_price", &self.input_token_price)
            .field("output_token_price", &self.output_token_price)
            .finish()
    }
}

impl std::fmt::Display for ModelInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.model_name)
    }
}

impl PartialEq for ModelInfo {
    fn eq(&self, other: &Self) -> bool {
        self.model_name == other.model_name
            && self.provider_id == other.provider_id
            && self.context_length == other.context_length
            && self.max_output_tokens == other.max_output_tokens
    }
}
