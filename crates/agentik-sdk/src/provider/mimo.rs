use crate::http::auth::AuthMethod;
use crate::model::{Model, ModelInfo};
use crate::provider::{LlmProvider, ProviderError};
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

pub struct MimoProvider;

impl MimoProvider {
    /// Return fully-configured preset models with the given API key and endpoint.
    pub fn preset_models(api_key: String, endpoint: Option<MimoEndpoint>) -> Vec<ModelInfo> {
        let base = endpoint.unwrap_or_default().base_url().to_string();
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
            // Pro series — 1M context, 128K output
            ModelInfo {
                model_name: MODEL_MIMO_V2_5_PRO.to_string(),
                provider_name: "mimo".to_string(),
                context_length: 1_000_000,
                max_output_tokens: 131_072,
                vision_ability: false,
                supports_function_calling: true,
                supports_streaming: true,
                supports_thinking: true,
                input_token_price: 1.0,
                output_token_price: 3.0,
                base_url: String::new(),
                api_key: String::new(),
                auth_method: AuthMethod::Anthropic,
            },
            ModelInfo {
                model_name: MODEL_MIMO_V2_PRO.to_string(),
                provider_name: "mimo".to_string(),
                context_length: 1_000_000,
                max_output_tokens: 131_072,
                vision_ability: false,
                supports_function_calling: true,
                supports_streaming: true,
                supports_thinking: true,
                input_token_price: 1.0,
                output_token_price: 3.0,
                base_url: String::new(),
                api_key: String::new(),
                auth_method: AuthMethod::Anthropic,
            },
            // Omni series — multi-modal understanding
            ModelInfo {
                model_name: MODEL_MIMO_V2_5.to_string(),
                provider_name: "mimo".to_string(),
                context_length: 1_000_000,
                max_output_tokens: 131_072,
                vision_ability: true,
                supports_function_calling: true,
                supports_streaming: true,
                supports_thinking: true,
                input_token_price: 0.4,
                output_token_price: 2.0,
                base_url: String::new(),
                api_key: String::new(),
                auth_method: AuthMethod::Anthropic,
            },
            ModelInfo {
                model_name: MODEL_MIMO_V2_OMNI.to_string(),
                provider_name: "mimo".to_string(),
                context_length: 262_144,
                max_output_tokens: 131_072,
                vision_ability: true,
                supports_function_calling: true,
                supports_streaming: true,
                supports_thinking: true,
                input_token_price: 0.4,
                output_token_price: 2.0,
                base_url: String::new(),
                api_key: String::new(),
                auth_method: AuthMethod::Anthropic,
            },
            // Flash series — lightweight, fast
            ModelInfo {
                model_name: MODEL_MIMO_V2_FLASH.to_string(),
                provider_name: "mimo".to_string(),
                context_length: 262_144,
                max_output_tokens: 65_536,
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
impl LlmProvider for MimoProvider {
    fn get_model(&self, model_name: &str, api_key: String) -> Result<Model, ProviderError> {
        let info = Self::preset_models(api_key, None)
            .into_iter()
            .find(|m| m.model_name == model_name)
            .ok_or_else(|| {
                ProviderError::ModelNotFound(ModelInfo {
                    model_name: model_name.to_string(),
                    provider_name: "mimo".to_string(),
                    base_url: String::new(),
                    api_key: String::new(),
                    auth_method: AuthMethod::Anthropic,
                    ..Default::default()
                })
            })?;
        Ok(Model::new(info)?)
    }

    fn add_models(&mut self, _model: Vec<ModelInfo>) {
        // No-op: MimoProvider is stateless.
    }

    async fn list_models(&self, _api_key: String) -> Result<Vec<Model>, ProviderError> {
        Ok(Self::preset_models(String::new(), None)
            .into_iter()
            .filter_map(|m| Model::new(m).ok())
            .collect())
    }
}
