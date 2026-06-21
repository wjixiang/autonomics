use serde::{Deserialize, Serialize};

use crate::http::auth::AuthMethod;

#[derive(Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub model_name: String,
    pub provider_name: String,
    pub context_length: u64,
    pub max_output_tokens: u64,
    pub vision_ability: bool,
    pub supports_function_calling: bool,
    pub supports_streaming: bool,
    pub supports_thinking: bool,
    pub input_token_price: f64,
    pub output_token_price: f64,
    // ── Connection config (formerly in ProviderInfo) ──
    pub base_url: String,
    pub api_key: String,
    pub auth_method: AuthMethod,
}

impl Default for ModelInfo {
    fn default() -> Self {
        Self {
            model_name: String::new(),
            provider_name: String::new(),
            context_length: 0,
            max_output_tokens: 0,
            vision_ability: false,
            supports_function_calling: false,
            supports_streaming: false,
            supports_thinking: false,
            input_token_price: 0.0,
            output_token_price: 0.0,
            base_url: String::new(),
            api_key: String::new(),
            auth_method: AuthMethod::default(),
        }
    }
}

impl std::fmt::Debug for ModelInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModelInfo")
            .field("model_name", &self.model_name)
            .field("provider_name", &self.provider_name)
            .field("context_length", &self.context_length)
            .field("max_output_tokens", &self.max_output_tokens)
            .field("vision_ability", &self.vision_ability)
            .field("supports_function_calling", &self.supports_function_calling)
            .field("supports_streaming", &self.supports_streaming)
            .field("supports_thinking", &self.supports_thinking)
            .field("base_url", &self.base_url)
            .field("api_key", &"[REDACTED]")
            .field("auth_method", &self.auth_method)
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
            && self.provider_name == other.provider_name
            && self.context_length == other.context_length
            && self.max_output_tokens == other.max_output_tokens
    }
}
