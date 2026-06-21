use crate::http::auth::AuthMethod;
use crate::model::{Model, ModelInfo};
use crate::provider::{LlmProvider, ProviderError};
use async_trait::async_trait;

pub const MODEL_MINIMAX_M2_7: &str = "MiniMax-M2.7";

pub struct MinimaxProvider;

impl MinimaxProvider {
    /// Return fully-configured preset models with the given API key and base URL.
    pub fn preset_models(api_key: String, base_url: String) -> Vec<ModelInfo> {
        Self::model_definitions()
            .into_iter()
            .map(|mut m| {
                m.base_url = base_url.clone();
                m.api_key = api_key.clone();
                m.auth_method = AuthMethod::Bearer;
                m
            })
            .collect()
    }

    fn model_definitions() -> Vec<ModelInfo> {
        vec![ModelInfo {
            model_name: MODEL_MINIMAX_M2_7.to_string(),
            provider_name: "minimax".to_string(),
            context_length: 1_000_000,
            max_output_tokens: 1000,
            vision_ability: true,
            supports_function_calling: true,
            supports_streaming: true,
            supports_thinking: true,
            input_token_price: 4.0,
            output_token_price: 16.0,
            base_url: String::new(),
            api_key: String::new(),
            auth_method: AuthMethod::Bearer,
        }]
    }
}

#[async_trait]
impl LlmProvider for MinimaxProvider {
    fn get_model(&self, model_name: &str, api_key: String) -> Result<Model, ProviderError> {
        // Minimax requires an explicit base_url — callers must use preset_models() directly.
        let _ = (model_name, api_key);
        Err(ProviderError::ModelNotFound(ModelInfo {
            model_name: model_name.to_string(),
            provider_name: "minimax".to_string(),
            base_url: String::new(),
            api_key: String::new(),
            auth_method: AuthMethod::Bearer,
            ..Default::default()
        }))
    }

    fn add_models(&mut self, _model: Vec<ModelInfo>) {
        // No-op: MinimaxProvider is stateless.
    }

    async fn list_models(&self, _api_key: String) -> Result<Vec<Model>, ProviderError> {
        Ok(vec![])
    }
}
