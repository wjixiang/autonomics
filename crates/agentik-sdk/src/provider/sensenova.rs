use crate::Anthropic;
use crate::config::ClientConfig;
use crate::config::LogLevel;
use crate::http::auth::AuthMethod;
use crate::model::{Model, ModelInfo};
use crate::provider::client::AnthropicApiClient;
use crate::provider::{LlmProvider, ProviderError, ProviderInfo};
use async_trait::async_trait;

pub const MODEL_SENSENOVA_6_7_FLASH_LITE: &str = "sensenova-6.7-flash-lite";
pub const MODEL_DEEPSEEK_V4_FLASH: &str = "deepseek-v4-flash";

pub const DEFAULT_BASE_URL: &str = "https://token.sensenova.cn";

pub struct SensenovaProvider {
    info: ProviderInfo,
}

impl SensenovaProvider {
    pub fn new(base_url: Option<String>, api_key: String) -> Self {
        let info = ProviderInfo {
            base_url: base_url.unwrap_or_else(|| DEFAULT_BASE_URL.to_string()),
            api_key,
            preset_models: Self::preset_models(),
        };

        Self { info }
    }

    pub fn preset_models() -> Vec<ModelInfo> {
        vec![
            // SenseNova 6.7 Flash-Lite — multimodal, 256K context, 64K output
            ModelInfo {
                model_name: MODEL_SENSENOVA_6_7_FLASH_LITE.to_string(),
                provider: "sensenova".to_string(),
                context_length: 262_144,
                max_output_tokens: 65_536,
                vision_ability: true,
                supports_function_calling: true,
                supports_streaming: true,
                supports_thinking: true,
                input_token_price: 0.0,
                output_token_price: 0.0,
            },
            // DeepSeek V4 Flash — 256K context, 64K output, thinking mode
            ModelInfo {
                model_name: MODEL_DEEPSEEK_V4_FLASH.to_string(),
                provider: "sensenova".to_string(),
                context_length: 262_144,
                max_output_tokens: 65_536,
                vision_ability: false,
                supports_function_calling: true,
                supports_streaming: true,
                supports_thinking: true,
                input_token_price: 0.0,
                output_token_price: 0.0,
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
            auth_method: AuthMethod::Bearer,
        }
    }
}

#[async_trait]
impl LlmProvider for SensenovaProvider {
    fn get_model(&self, model_name: &str) -> Result<Model, ProviderError> {
        let existed_model = self
            .info
            .preset_models
            .iter()
            .find(|i| i.model_name == model_name)
            .ok_or_else(|| {
                ProviderError::ModelNotFound(ModelInfo {
                    model_name: model_name.to_string(),
                    provider: "sensenova".to_string(),
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
