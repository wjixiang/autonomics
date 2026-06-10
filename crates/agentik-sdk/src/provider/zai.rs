use crate::Anthropic;
use crate::config::ClientConfig;
use crate::config::LogLevel;
use crate::http::auth::AuthMethod;
use crate::model::{Model, ModelInfo};
use crate::provider::client::AnthropicApiClient;
use crate::provider::{LlmProvider, ProviderError, ProviderInfo};
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

pub struct ZaiProvider {
    info: ProviderInfo,
}

impl ZaiProvider {
    pub fn new(endpoint: Option<ZaiEndpoint>, api_key: String) -> Self {
        let info = ProviderInfo {
            base_url: endpoint.unwrap_or_default().base_url().to_string(),
            api_key,
            preset_models: Self::preset_models(),
        };

        Self { info }
    }

    pub fn preset_models() -> Vec<ModelInfo> {
        vec![
            // ── Flagship series (200K context, 32K output) ───────────────
            ModelInfo {
                model_name: MODEL_GLM_5_1.to_string(),
                provider: "zai".to_string(),
                context_length: 200_000,
                max_output_tokens: 32_000,
                vision_ability: false,
                supports_function_calling: true,
                supports_streaming: true,
                supports_thinking: true,
                input_token_price: 2.0,
                output_token_price: 8.0,
            },
            ModelInfo {
                model_name: MODEL_GLM_5.to_string(),
                provider: "zai".to_string(),
                context_length: 200_000,
                max_output_tokens: 32_000,
                vision_ability: false,
                supports_function_calling: true,
                supports_streaming: true,
                supports_thinking: true,
                input_token_price: 2.0,
                output_token_price: 8.0,
            },
            ModelInfo {
                model_name: MODEL_GLM_5_TURBO.to_string(),
                provider: "zai".to_string(),
                context_length: 200_000,
                max_output_tokens: 32_000,
                vision_ability: false,
                supports_function_calling: true,
                supports_streaming: true,
                supports_thinking: false,
                input_token_price: 1.0,
                output_token_price: 3.0,
            },
            // ── 4.x flagship series (128K context, 16K output) ───────────
            ModelInfo {
                model_name: MODEL_GLM_4_7.to_string(),
                provider: "zai".to_string(),
                context_length: 128_000,
                max_output_tokens: 16_000,
                vision_ability: false,
                supports_function_calling: true,
                supports_streaming: true,
                supports_thinking: true,
                input_token_price: 2.0,
                output_token_price: 8.0,
            },
            ModelInfo {
                model_name: MODEL_GLM_4_6.to_string(),
                provider: "zai".to_string(),
                context_length: 128_000,
                max_output_tokens: 16_000,
                vision_ability: false,
                supports_function_calling: true,
                supports_streaming: true,
                supports_thinking: true,
                input_token_price: 1.0,
                output_token_price: 4.0,
            },
            ModelInfo {
                model_name: MODEL_GLM_4_5.to_string(),
                provider: "zai".to_string(),
                context_length: 128_000,
                max_output_tokens: 16_000,
                vision_ability: false,
                supports_function_calling: true,
                supports_streaming: true,
                supports_thinking: true,
                input_token_price: 1.0,
                output_token_price: 4.0,
            },
            // ── Air / mid-tier ───────────────────────────────────────────
            ModelInfo {
                model_name: MODEL_GLM_4_5_AIR.to_string(),
                provider: "zai".to_string(),
                context_length: 128_000,
                max_output_tokens: 16_000,
                vision_ability: false,
                supports_function_calling: true,
                supports_streaming: true,
                supports_thinking: false,
                input_token_price: 0.3,
                output_token_price: 1.2,
            },
            // ── Flash / lightweight ──────────────────────────────────────
            ModelInfo {
                model_name: MODEL_GLM_4_7_FLASH.to_string(),
                provider: "zai".to_string(),
                context_length: 128_000,
                max_output_tokens: 16_000,
                vision_ability: false,
                supports_function_calling: true,
                supports_streaming: true,
                supports_thinking: false,
                input_token_price: 0.1,
                output_token_price: 0.1,
            },
            ModelInfo {
                model_name: MODEL_GLM_4_FLASH.to_string(),
                provider: "zai".to_string(),
                context_length: 128_000,
                max_output_tokens: 16_000,
                vision_ability: false,
                supports_function_calling: true,
                supports_streaming: true,
                supports_thinking: false,
                input_token_price: 0.1,
                output_token_price: 0.1,
            },
            // ── Vision series (64K context) ─────────────────────────────
            ModelInfo {
                model_name: MODEL_GLM_4_1V_THINKING_FLASH.to_string(),
                provider: "zai".to_string(),
                context_length: 64_000,
                max_output_tokens: 8_000,
                vision_ability: true,
                supports_function_calling: true,
                supports_streaming: true,
                supports_thinking: true,
                input_token_price: 0.5,
                output_token_price: 0.5,
            },
            ModelInfo {
                model_name: MODEL_GLM_4_6V_FLASH.to_string(),
                provider: "zai".to_string(),
                context_length: 64_000,
                max_output_tokens: 8_000,
                vision_ability: true,
                supports_function_calling: true,
                supports_streaming: true,
                supports_thinking: false,
                input_token_price: 0.5,
                output_token_price: 0.5,
            },
            ModelInfo {
                model_name: MODEL_GLM_4V_FLASH.to_string(),
                provider: "zai".to_string(),
                context_length: 64_000,
                max_output_tokens: 8_000,
                vision_ability: true,
                supports_function_calling: true,
                supports_streaming: true,
                supports_thinking: false,
                input_token_price: 0.1,
                output_token_price: 0.1,
            },
        ]
    }

    fn build_client_config(&self) -> ClientConfig {
        ClientConfig {
            api_key: self.info.api_key.clone(),
            base_url: self.info.base_url.clone(),
            timeout: core::time::Duration::from_secs(30),
            max_retries: 3,
            log_level: LogLevel::Debug,
            // Zhipu's open platform uses standard HTTP Bearer auth.
            auth_method: AuthMethod::Bearer,
        }
    }
}

#[async_trait]
impl LlmProvider for ZaiProvider {
    fn get_model(&self, model_name: &str) -> Result<Model, ProviderError> {
        let existed_model = self
            .info
            .preset_models
            .iter()
            .find(|i| i.model_name == model_name)
            .ok_or_else(|| {
                ProviderError::ModelNotFound(ModelInfo {
                    model_name: model_name.to_string(),
                    provider: "zai".to_string(),
                    ..Default::default()
                })
            })?;

        let client = AnthropicApiClient::new(Anthropic::with_config(self.build_client_config())?);
        Ok(Model::new(existed_model.clone(), client))
    }

    fn add_models(&mut self, model: Vec<ModelInfo>) {
        self.info.preset_models.extend(model);
    }

    async fn list_models(&self) -> Result<Vec<Model>, ProviderError> {
        let client = Anthropic::with_config(self.build_client_config())?;
        let model_list = client.models().list(None).await?;

        let mut models = Vec::with_capacity(model_list.data.len());
        for model_obj in &model_list.data {
            if let Some(model_info) = self
                .info
                .preset_models
                .iter()
                .find(|i| i.model_name == model_obj.id)
            {
                let api_client =
                    AnthropicApiClient::new(Anthropic::with_config(self.build_client_config())?);
                models.push(Model::new(model_info.clone(), api_client));
            }
        }
        Ok(models)
    }
}
