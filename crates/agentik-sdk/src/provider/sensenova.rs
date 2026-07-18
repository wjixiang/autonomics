use crate::model::ModelInfo;
use crate::model::ProviderType;

pub const MODEL_SENSENOVA_6_7_FLASH_LITE: &str = "sensenova-6.7-flash-lite";
pub const MODEL_DEEPSEEK_V4_FLASH: &str = "deepseek-v4-flash";

pub const DEFAULT_BASE_URL: &str = "https://token.sensenova.cn";

/// Provider type key used by `ProviderConfig::provider_type`.
pub const PROVIDER_TYPE: ProviderType = ProviderType::Sensenova;

pub struct SensenovaProvider;

impl SensenovaProvider {
    /// Preset model catalogue for the sensenova provider type — metadata only.
    pub fn preset_models() -> Vec<ModelInfo> {
        Self::model_definitions()
    }

    fn model_definitions() -> Vec<ModelInfo> {
        vec![
            // SenseNova 6.7 Flash-Lite — multimodal, 256K context, 64K output
            ModelInfo {
                model_name: MODEL_SENSENOVA_6_7_FLASH_LITE.to_string(),
                provider_id: uuid::Uuid::nil(),
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
                provider_id: uuid::Uuid::nil(),
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
}
