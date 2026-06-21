use crate::http::auth::AuthMethod;
use crate::model::{Model, ModelInfo};
use crate::provider::{LlmProvider, ProviderError};
use async_trait::async_trait;

// ─── Model IDs ──────────────────────────────────────────────────────────────
// Current generation
pub const MODEL_DEEPSEEK_V4_PRO: &str = "deepseek-v4-pro";
pub const MODEL_DEEPSEEK_V4_FLASH: &str = "deepseek-v4-flash";
// Deprecated aliases — still accepted, mapped server-side to v4-flash.
pub const MODEL_DEEPSEEK_CHAT: &str = "deepseek-chat";
pub const MODEL_DEEPSEEK_REASONER: &str = "deepseek-reasoner";

pub const DEFAULT_BASE_URL: &str = "https://api.deepseek.com/anthropic";

pub struct DeepseekProvider;

impl DeepseekProvider {
    /// Return fully-configured preset models with the given API key.
    pub fn preset_models(api_key: String, base_url: Option<String>) -> Vec<ModelInfo> {
        let base = base_url.unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
        Self::model_definitions()
            .into_iter()
            .map(|mut m| {
                m.base_url = base.clone();
                m.api_key = api_key.clone();
                m.auth_method = AuthMethod::Anthropic;
                m
            })
            .collect()
    }

    fn model_definitions() -> Vec<ModelInfo> {
        vec![
            // ── Current generation ───────────────────────────────────────
            // V4 Pro — flagship reasoning model.
            ModelInfo {
                model_name: MODEL_DEEPSEEK_V4_PRO.to_string(),
                provider_name: "deepseek".to_string(),
                context_length: 128_000,
                max_output_tokens: 32_000,
                vision_ability: false,
                supports_function_calling: true,
                supports_streaming: true,
                supports_thinking: true,
                input_token_price: 0.5,
                output_token_price: 2.0,
                base_url: String::new(),
                api_key: String::new(),
                auth_method: AuthMethod::Anthropic,
            },
            // V4 Flash — fast, low-cost, still supports thinking.
            ModelInfo {
                model_name: MODEL_DEEPSEEK_V4_FLASH.to_string(),
                provider_name: "deepseek".to_string(),
                context_length: 128_000,
                max_output_tokens: 32_000,
                vision_ability: false,
                supports_function_calling: true,
                supports_streaming: true,
                supports_thinking: true,
                input_token_price: 0.1,
                output_token_price: 0.3,
                base_url: String::new(),
                api_key: String::new(),
                auth_method: AuthMethod::Anthropic,
            },
            // ── Deprecated aliases (kept for backwards compatibility) ─────
            // deepseek-chat — non-thinking mode of v4-flash.
            ModelInfo {
                model_name: MODEL_DEEPSEEK_CHAT.to_string(),
                provider_name: "deepseek".to_string(),
                context_length: 64_000,
                max_output_tokens: 8_000,
                vision_ability: false,
                supports_function_calling: true,
                supports_streaming: true,
                supports_thinking: false,
                input_token_price: 0.1,
                output_token_price: 0.3,
                base_url: String::new(),
                api_key: String::new(),
                auth_method: AuthMethod::Anthropic,
            },
            // deepseek-reasoner — thinking mode of v4-flash.
            ModelInfo {
                model_name: MODEL_DEEPSEEK_REASONER.to_string(),
                provider_name: "deepseek".to_string(),
                context_length: 64_000,
                max_output_tokens: 8_000,
                vision_ability: false,
                supports_function_calling: true,
                supports_streaming: true,
                supports_thinking: true,
                input_token_price: 0.1,
                output_token_price: 0.3,
                base_url: String::new(),
                api_key: String::new(),
                auth_method: AuthMethod::Anthropic,
            },
        ]
    }
}

#[async_trait]
impl LlmProvider for DeepseekProvider {
    fn get_model(&self, model_name: &str, api_key: String) -> Result<Model, ProviderError> {
        let info = Self::preset_models(api_key, None)
            .into_iter()
            .find(|m| m.model_name == model_name)
            .ok_or_else(|| {
                ProviderError::ModelNotFound(ModelInfo {
                    model_name: model_name.to_string(),
                    provider_name: "deepseek".to_string(),
                    base_url: String::new(),
                    api_key: String::new(),
                    auth_method: AuthMethod::Anthropic,
                    ..Default::default()
                })
            })?;
        Ok(Model::new(info)?)
    }

    fn add_models(&mut self, _model: Vec<ModelInfo>) {
        // No-op: DeepseekProvider is stateless, models are built on demand.
    }

    async fn list_models(&self, _api_key: String) -> Result<Vec<Model>, ProviderError> {
        // Stateless preset list — no remote discovery.
        Ok(Self::preset_models(String::new(), None)
            .into_iter()
            .filter_map(|m| Model::new(m).ok())
            .collect())
    }
}
