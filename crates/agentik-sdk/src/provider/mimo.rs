use crate::model::ModelInfo;
use crate::model::ProviderType;

pub const MODEL_MIMO_V2_5_PRO: &str = "mimo-v2.5-pro";
pub const MODEL_MIMO_V2_PRO: &str = "mimo-v2-pro";
pub const MODEL_MIMO_V2_5: &str = "mimo-v2.5";
pub const MODEL_MIMO_V2_OMNI: &str = "mimo-v2-omni";
pub const MODEL_MIMO_V2_FLASH: &str = "mimo-v2-flash";

/// Provider type key used by `ProviderConfig::provider_type`.
pub const PROVIDER_TYPE: ProviderType = ProviderType::Mimo;

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
    /// Preset model catalogue for the mimo provider type — metadata only.
    pub fn preset_models() -> Vec<ModelInfo> {
        Self::model_definitions()
    }

    fn model_definitions() -> Vec<ModelInfo> {
        vec![
            // Pro series — 1M context, 128K output
            ModelInfo {
                model_name: MODEL_MIMO_V2_5_PRO.to_string(),
                provider_id: uuid::Uuid::nil(),
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
                provider_id: uuid::Uuid::nil(),
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
                provider_id: uuid::Uuid::nil(),
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
                provider_id: uuid::Uuid::nil(),
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
                provider_id: uuid::Uuid::nil(),
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
}
