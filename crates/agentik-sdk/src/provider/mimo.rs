use crate::Anthropic;
use crate::config::ClientConfig;
use crate::config::LogLevel;
use crate::http::auth::AuthMethod;
use crate::model::{Model, ModelInfo};
use crate::provider::client::AnthropicApiClient;
use crate::provider::{LlmProvider, ProviderError, ProviderInfo};
use async_trait::async_trait;

pub const MODEL_MIMO_V2_5_PRO: &str = "mimo-v2.5-pro";
pub const MODEL_MIMO_V2_PRO: &str = "mimo-v2-pro";
pub const MODEL_MIMO_V2_5: &str = "mimo-v2.5";
pub const MODEL_MIMO_V2_OMNI: &str = "mimo-v2-omni";
pub const MODEL_MIMO_V2_FLASH: &str = "mimo-v2-flash";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenPlanRegion {
    China,
    Eur,
    Sgp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MimoEndpoint {
    Api,
    TokenPlan(TokenPlanRegion),
}

impl MimoEndpoint {
    pub fn base_url(self) -> &'static str {
        match self {
            MimoEndpoint::Api => "https://api.xiaomimimo.com/anthropic",
            MimoEndpoint::TokenPlan(TokenPlanRegion::China) => {
                "https://token-plan-cn.xiaomimimo.com/anthropic"
            }
            MimoEndpoint::TokenPlan(TokenPlanRegion::Eur) => {
                "https://token-plan-eur.xiaomimimo.com/anthropic"
            }
            MimoEndpoint::TokenPlan(TokenPlanRegion::Sgp) => {
                "https://token-plan-sgp.xiaomimimo.com/anthropic"
            }
        }
    }
}

impl Default for MimoEndpoint {
    fn default() -> Self {
        MimoEndpoint::TokenPlan(TokenPlanRegion::China)
    }
}

pub struct MimoProvider {
    info: ProviderInfo,
}

impl MimoProvider {
    pub fn new(endpoint: Option<MimoEndpoint>, api_key: String) -> Self {
        let info = ProviderInfo {
            base_url: endpoint.unwrap_or_default().base_url().to_string(),
            api_key,
            preset_models: Self::preset_models(),
        };

        Self { info }
    }

    pub fn preset_models() -> Vec<ModelInfo> {
        vec![
            // Pro series — 1M context, 128K output
            ModelInfo {
                model_name: MODEL_MIMO_V2_5_PRO.to_string(),
                provider: "mimo".to_string(),
                context_length: 1_000_000,
                max_output_tokens: 131_072,
                vision_ability: false,
                supports_function_calling: true,
                supports_streaming: true,
                supports_thinking: true,
                input_token_price: 1.0,
                output_token_price: 3.0,
            },
            ModelInfo {
                model_name: MODEL_MIMO_V2_PRO.to_string(),
                provider: "mimo".to_string(),
                context_length: 1_000_000,
                max_output_tokens: 131_072,
                vision_ability: false,
                supports_function_calling: true,
                supports_streaming: true,
                supports_thinking: true,
                input_token_price: 1.0,
                output_token_price: 3.0,
            },
            // Omni series — multi-modal understanding
            ModelInfo {
                model_name: MODEL_MIMO_V2_5.to_string(),
                provider: "mimo".to_string(),
                context_length: 1_000_000,
                max_output_tokens: 131_072,
                vision_ability: true,
                supports_function_calling: true,
                supports_streaming: true,
                supports_thinking: true,
                input_token_price: 0.4,
                output_token_price: 2.0,
            },
            ModelInfo {
                model_name: MODEL_MIMO_V2_OMNI.to_string(),
                provider: "mimo".to_string(),
                context_length: 262_144,
                max_output_tokens: 131_072,
                vision_ability: true,
                supports_function_calling: true,
                supports_streaming: true,
                supports_thinking: true,
                input_token_price: 0.4,
                output_token_price: 2.0,
            },
            // Flash series — lightweight, fast
            ModelInfo {
                model_name: MODEL_MIMO_V2_FLASH.to_string(),
                provider: "mimo".to_string(),
                context_length: 262_144,
                max_output_tokens: 65_536,
                vision_ability: false,
                supports_function_calling: true,
                supports_streaming: true,
                supports_thinking: true,
                input_token_price: 0.1,
                output_token_price: 0.3,
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
            auth_method: AuthMethod::Anthropic,
        }
    }
}

#[async_trait]
impl LlmProvider for MimoProvider {
    fn get_model(&self, model_name: &str) -> Result<Model, ProviderError> {
        let existed_model = self
            .info
            .preset_models
            .iter()
            .find(|i| i.model_name == model_name)
            .ok_or_else(|| {
                ProviderError::ModelNotFound(ModelInfo {
                    model_name: model_name.to_string(),
                    provider: "mimo".to_string(),
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
