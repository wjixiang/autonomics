use crate::http::auth::AuthMethod;
use crate::model::{Model, ModelInfo};
use crate::provider::{LlmProvider, ProviderError};
use async_trait::async_trait;

// ─── Model IDs ──────────────────────────────────────────────────────────────
// Flagship series
pub const MODEL_GLM_5_1: &str = "glm-5.1";
pub const MODEL_GLM_5: &str = "glm-5";
pub const MODEL_GLM_5_TURBO: &str = "glm-5-turbo";
// 4.x series
pub const MODEL_GLM_4_7: &str = "glm-4.7";
pub const MODEL_GLM_4_6: &str = "glm-4.6";
pub const MODEL_GLM_4_5: &str = "glm-4.5";
pub const MODEL_GLM_4_5_AIR: &str = "glm-4.5-air";
// Flash / lightweight
pub const MODEL_GLM_4_7_FLASH: &str = "glm-4.7-flash";
pub const MODEL_GLM_4_FLASH: &str = "glm-4-flash";
// Vision-capable
pub const MODEL_GLM_4_1V_THINKING_FLASH: &str = "glm-4.1v-thinking-flash";
pub const MODEL_GLM_4_6V_FLASH: &str = "glm-4.6v-flash";
pub const MODEL_GLM_4V_FLASH: &str = "glm-4v-flash";

/// Endpoint selector for the Zhipu / BigModel open platform.
///
/// The general `Api` endpoint exposes the full model catalogue. The
/// `TokenPlan` endpoint is dedicated to the [GLM 编码套餐](https://bigmodel.cn)
/// (coding token-plan) and is only valid for coding scenarios.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ZaiEndpoint {
    /// General Open API — `https://open.bigmodel.cn/api/paas/v4`
    Api,
    /// GLM coding token-plan — `https://open.bigmodel.cn/api/coding/paas/v4`
    /// (coding token-plan) and is only valid for coding scenarios.
    #[default]
    TokenPlan,
}

impl ZaiEndpoint {
    pub fn base_url(self) -> &'static str {
        match self {
            ZaiEndpoint::Api => "https://open.bigmodel.cn/api/paas/v4",
            ZaiEndpoint::TokenPlan => "https://open.bigmodel.cn/api/coding/paas/v4",
        }
    }
}

pub struct ZaiProvider;

impl ZaiProvider {
    /// Return fully-configured preset models with the given API key and endpoint.
    pub fn preset_models(api_key: String, endpoint: Option<ZaiEndpoint>) -> Vec<ModelInfo> {
        let base = endpoint.unwrap_or_default().base_url().to_string();
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
            // ── Flagship series (200K context, 32K output) ───────────────
            ModelInfo {
                model_name: MODEL_GLM_5_1.to_string(),
                provider_name: "zai".to_string(),
                context_length: 200_000,
                max_output_tokens: 32_000,
                vision_ability: false,
                supports_function_calling: true,
                supports_streaming: true,
                supports_thinking: true,
                input_token_price: 2.0,
                output_token_price: 8.0,
                base_url: String::new(),
                api_key: String::new(),
                auth_method: AuthMethod::Bearer,
            },
            ModelInfo {
                model_name: MODEL_GLM_5.to_string(),
                provider_name: "zai".to_string(),
                context_length: 200_000,
                max_output_tokens: 32_000,
                vision_ability: false,
                supports_function_calling: true,
                supports_streaming: true,
                supports_thinking: true,
                input_token_price: 2.0,
                output_token_price: 8.0,
                base_url: String::new(),
                api_key: String::new(),
                auth_method: AuthMethod::Bearer,
            },
            ModelInfo {
                model_name: MODEL_GLM_5_TURBO.to_string(),
                provider_name: "zai".to_string(),
                context_length: 200_000,
                max_output_tokens: 32_000,
                vision_ability: false,
                supports_function_calling: true,
                supports_streaming: true,
                supports_thinking: false,
                input_token_price: 1.0,
                output_token_price: 3.0,
                base_url: String::new(),
                api_key: String::new(),
                auth_method: AuthMethod::Bearer,
            },
            // ── 4.x flagship series (128K context, 16K output) ───────────
            ModelInfo {
                model_name: MODEL_GLM_4_7.to_string(),
                provider_name: "zai".to_string(),
                context_length: 128_000,
                max_output_tokens: 16_000,
                vision_ability: false,
                supports_function_calling: true,
                supports_streaming: true,
                supports_thinking: true,
                input_token_price: 2.0,
                output_token_price: 8.0,
                base_url: String::new(),
                api_key: String::new(),
                auth_method: AuthMethod::Bearer,
            },
            ModelInfo {
                model_name: MODEL_GLM_4_6.to_string(),
                provider_name: "zai".to_string(),
                context_length: 128_000,
                max_output_tokens: 16_000,
                vision_ability: false,
                supports_function_calling: true,
                supports_streaming: true,
                supports_thinking: true,
                input_token_price: 1.0,
                output_token_price: 4.0,
                base_url: String::new(),
                api_key: String::new(),
                auth_method: AuthMethod::Bearer,
            },
            ModelInfo {
                model_name: MODEL_GLM_4_5.to_string(),
                provider_name: "zai".to_string(),
                context_length: 128_000,
                max_output_tokens: 16_000,
                vision_ability: false,
                supports_function_calling: true,
                supports_streaming: true,
                supports_thinking: true,
                input_token_price: 1.0,
                output_token_price: 4.0,
                base_url: String::new(),
                api_key: String::new(),
                auth_method: AuthMethod::Bearer,
            },
            // ── Air / mid-tier ───────────────────────────────────────────
            ModelInfo {
                model_name: MODEL_GLM_4_5_AIR.to_string(),
                provider_name: "zai".to_string(),
                context_length: 128_000,
                max_output_tokens: 16_000,
                vision_ability: false,
                supports_function_calling: true,
                supports_streaming: true,
                supports_thinking: false,
                input_token_price: 0.3,
                output_token_price: 1.2,
                base_url: String::new(),
                api_key: String::new(),
                auth_method: AuthMethod::Bearer,
            },
            // ── Flash / lightweight ──────────────────────────────────────
            ModelInfo {
                model_name: MODEL_GLM_4_7_FLASH.to_string(),
                provider_name: "zai".to_string(),
                context_length: 128_000,
                max_output_tokens: 16_000,
                vision_ability: false,
                supports_function_calling: true,
                supports_streaming: true,
                supports_thinking: false,
                input_token_price: 0.1,
                output_token_price: 0.1,
                base_url: String::new(),
                api_key: String::new(),
                auth_method: AuthMethod::Bearer,
            },
            ModelInfo {
                model_name: MODEL_GLM_4_FLASH.to_string(),
                provider_name: "zai".to_string(),
                context_length: 128_000,
                max_output_tokens: 16_000,
                vision_ability: false,
                supports_function_calling: true,
                supports_streaming: true,
                supports_thinking: false,
                input_token_price: 0.1,
                output_token_price: 0.1,
                base_url: String::new(),
                api_key: String::new(),
                auth_method: AuthMethod::Bearer,
            },
            // ── Vision series (64K context) ─────────────────────────────
            ModelInfo {
                model_name: MODEL_GLM_4_1V_THINKING_FLASH.to_string(),
                provider_name: "zai".to_string(),
                context_length: 64_000,
                max_output_tokens: 8_000,
                vision_ability: true,
                supports_function_calling: true,
                supports_streaming: true,
                supports_thinking: true,
                input_token_price: 0.5,
                output_token_price: 0.5,
                base_url: String::new(),
                api_key: String::new(),
                auth_method: AuthMethod::Bearer,
            },
            ModelInfo {
                model_name: MODEL_GLM_4_6V_FLASH.to_string(),
                provider_name: "zai".to_string(),
                context_length: 64_000,
                max_output_tokens: 8_000,
                vision_ability: true,
                supports_function_calling: true,
                supports_streaming: true,
                supports_thinking: false,
                input_token_price: 0.5,
                output_token_price: 0.5,
                base_url: String::new(),
                api_key: String::new(),
                auth_method: AuthMethod::Bearer,
            },
            ModelInfo {
                model_name: MODEL_GLM_4V_FLASH.to_string(),
                provider_name: "zai".to_string(),
                context_length: 64_000,
                max_output_tokens: 8_000,
                vision_ability: true,
                supports_function_calling: true,
                supports_streaming: true,
                supports_thinking: false,
                input_token_price: 0.1,
                output_token_price: 0.1,
                base_url: String::new(),
                api_key: String::new(),
                auth_method: AuthMethod::Bearer,
            },
        ]
    }
}

#[async_trait]
impl LlmProvider for ZaiProvider {
    fn get_model(&self, model_name: &str, api_key: String) -> Result<Model, ProviderError> {
        let info = Self::preset_models(api_key, None)
            .into_iter()
            .find(|m| m.model_name == model_name)
            .ok_or_else(|| {
                ProviderError::ModelNotFound(ModelInfo {
                    model_name: model_name.to_string(),
                    provider_name: "zai".to_string(),
                    base_url: String::new(),
                    api_key: String::new(),
                    auth_method: AuthMethod::Bearer,
                    ..Default::default()
                })
            })?;
        Ok(Model::new(info)?)
    }

    fn add_models(&mut self, _model: Vec<ModelInfo>) {
        // No-op: ZaiProvider is stateless.
    }

    async fn list_models(&self, _api_key: String) -> Result<Vec<Model>, ProviderError> {
        Ok(Self::preset_models(String::new(), None)
            .into_iter()
            .filter_map(|m| Model::new(m).ok())
            .collect())
    }
}
