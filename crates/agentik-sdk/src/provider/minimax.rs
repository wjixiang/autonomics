use crate::model::ModelInfo;
use crate::model::ProviderType;

pub const MODEL_MINIMAX_M2_7: &str = "MiniMax-M2.7";

/// Provider type key used by `ProviderConfig::provider_type`.
pub const PROVIDER_TYPE: ProviderType = ProviderType::Minimax;

/// Minimax has no fixed endpoint preset — a `base_url` must be supplied when
/// creating the provider instance.
pub const DEFAULT_BASE_URL: &str = "";

pub struct MinimaxProvider;

impl MinimaxProvider {
    /// Preset model catalogue for the minimax provider type — metadata only.
    pub fn preset_models() -> Vec<ModelInfo> {
        Self::model_definitions()
    }

    fn model_definitions() -> Vec<ModelInfo> {
        vec![ModelInfo {
            model_name: MODEL_MINIMAX_M2_7.to_string(),
            provider_id: uuid::Uuid::nil(),
            context_length: 1_000_000,
            max_output_tokens: 1000,
            vision_ability: true,
            supports_function_calling: true,
            supports_streaming: true,
            supports_thinking: true,
            input_token_price: 4.0,
            output_token_price: 16.0,
        }]
    }
}
