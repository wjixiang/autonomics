use crate::model::model_info::ModelInfoBuilder;
use crate::model::ModelInfo;
use crate::model::ProviderType;
use crate::provider::ProviderPreset;

pub const MODEL_MINIMAX_M2_7: &str = "MiniMax-M2.7";

/// Minimax has no fixed endpoint preset — a `base_url` must be supplied when
/// creating the provider instance.
pub const DEFAULT_BASE_URL: &str = "";

pub struct MinimaxProvider;

impl ProviderPreset for MinimaxProvider {
    fn provider_type() -> ProviderType {
        ProviderType::Minimax
    }
    fn preset_models() -> Vec<ModelInfo> {
        Self::model_definitions()
    }
    fn default_base_url() -> &'static str {
        DEFAULT_BASE_URL
    }
}

impl MinimaxProvider {
    /// Preset model catalogue for the minimax provider type — metadata only.
    pub fn preset_models() -> Vec<ModelInfo> {
        <Self as ProviderPreset>::preset_models()
    }

    fn model_definitions() -> Vec<ModelInfo> {
        vec![ModelInfoBuilder::new(MODEL_MINIMAX_M2_7)
            .context(1_000_000, 1000)
            .capabilities(true, true, true, true)
            .pricing(4.0, 16.0)
            .build()]
    }
}
