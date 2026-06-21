use crate::http::auth::AuthMethod;
use crate::model::{Model, ModelInfo};
use crate::provider::{LlmProvider, ProviderError};
use async_trait::async_trait;

pub const MODEL_SENSENOVA_6_7_FLASH_LITE: &str = "sensenova-6.7-flash-lite";
pub const MODEL_DEEPSEEK_V4_FLASH: &str = "deepseek-v4-flash";

pub const DEFAULT_BASE_URL: &str = "https://token.sensenova.cn";

pub struct SensenovaProvider;

impl SensenovaProvider {
    /// Return fully-configured preset models with the given API key.
    pub fn preset_models(api_key: String, base_url: Option<String>) -> Vec<ModelInfo> {
        let base = base_url.unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
        Self::model_definitions()
            .into_iter()
            .map(|mut m| {
                m.base_url = base.clone();
                m.api_key = api_key.clone();
                m.auth_method = AuthMethod::Bearer;
                m
            })
            .collect()
    }

    fn model_definitions() -> Vec<ModelInfo> {
        vec![
            // SenseNova 6.7 Flash-Lite — multimodal, 256K context, 64K output
            ModelInfo {
                model_name: MODEL_SENSENOVA_6_7_FLASH_LITE.to_string(),
                provider_name: "sensenova".to_string(),
                context_length: 262_144,
                max_output_tokens: 65_536,
                vision_ability: true,
                supports_function_calling: true,
                supports_streaming: true,
                supports_thinking: true,
                input_token_price: 0.0,
                output_token_price: 0.0,
                base_url: String::new(),
                api_key: String::new(),
                auth_method: AuthMethod::Bearer,
            },
            // DeepSeek V4 Flash — 256K context, 64K output, thinking mode
            ModelInfo {
                model_name: MODEL_DEEPSEEK_V4_FLASH.to_string(),
                provider_name: "sensenova".to_string(),
                context_length: 262_144,
                max_output_tokens: 65_536,
                vision_ability: false,
                supports_function_calling: true,
                supports_streaming: true,
                supports_thinking: true,
                input_token_price: 0.0,
                output_token_price: 0.0,
                base_url: String::new(),
                api_key: String::new(),
                auth_method: AuthMethod::Bearer,
            },
        ]
    }
}

#[async_trait]
impl LlmProvider for SensenovaProvider {
    fn get_model(&self, model_name: &str, api_key: String) -> Result<Model, ProviderError> {
        let info = Self::preset_models(api_key, None)
            .into_iter()
            .find(|m| m.model_name == model_name)
            .ok_or_else(|| {
                ProviderError::ModelNotFound(ModelInfo {
                    model_name: model_name.to_string(),
                    provider_name: "sensenova".to_string(),
                    base_url: String::new(),
                    api_key: String::new(),
                    auth_method: AuthMethod::Bearer,
                    ..Default::default()
                })
            })?;
        Ok(Model::new(info)?)
    }

    fn add_models(&mut self, _model: Vec<ModelInfo>) {
        // No-op: SensenovaProvider is stateless.
    }

    async fn list_models(&self, _api_key: String) -> Result<Vec<Model>, ProviderError> {
        Ok(Self::preset_models(String::new(), None)
            .into_iter()
            .filter_map(|m| Model::new(m).ok())
            .collect())
    }
}
