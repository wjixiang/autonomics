use crate::model::ModelInfo;
use crate::model::ProviderType;
use crate::model::model_info::ModelInfoBuilder;
use crate::provider::ProviderPreset;

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

impl ProviderPreset for ZaiProvider {
    fn provider_type() -> ProviderType {
        ProviderType::Zai
    }
    fn preset_models() -> Vec<ModelInfo> {
        Self::model_definitions()
    }
    fn default_base_url() -> &'static str {
        // Default to the token-plan endpoint (coding scenarios).
        ZaiEndpoint::default().base_url()
    }
}

impl ZaiProvider {
    /// Preset model catalogue for the zai provider type — metadata only.
    pub fn preset_models() -> Vec<ModelInfo> {
        <Self as ProviderPreset>::preset_models()
    }

    fn model_definitions() -> Vec<ModelInfo> {
        vec![
            // ── Flagship series (200K context, 32K output) ───────────────
            ModelInfoBuilder::new(MODEL_GLM_5_1)
                .context(200_000, 32_000)
                .capabilities(false, true, true, true)
                .pricing(2.0, 8.0)
                .build(),
            ModelInfoBuilder::new(MODEL_GLM_5)
                .context(200_000, 32_000)
                .capabilities(false, true, true, true)
                .pricing(2.0, 8.0)
                .build(),
            ModelInfoBuilder::new(MODEL_GLM_5_TURBO)
                .context(200_000, 32_000)
                .capabilities(false, true, true, false)
                .pricing(1.0, 3.0)
                .build(),
            // ── 4.x flagship series (128K context, 16K output) ───────────
            ModelInfoBuilder::new(MODEL_GLM_4_7)
                .context(128_000, 16_000)
                .capabilities(false, true, true, true)
                .pricing(2.0, 8.0)
                .build(),
            ModelInfoBuilder::new(MODEL_GLM_4_6)
                .context(128_000, 16_000)
                .capabilities(false, true, true, true)
                .pricing(1.0, 4.0)
                .build(),
            ModelInfoBuilder::new(MODEL_GLM_4_5)
                .context(128_000, 16_000)
                .capabilities(false, true, true, true)
                .pricing(1.0, 4.0)
                .build(),
            // ── Air / mid-tier ───────────────────────────────────────────
            ModelInfoBuilder::new(MODEL_GLM_4_5_AIR)
                .context(128_000, 16_000)
                .capabilities(false, true, true, false)
                .pricing(0.3, 1.2)
                .build(),
            // ── Flash / lightweight ──────────────────────────────────────
            ModelInfoBuilder::new(MODEL_GLM_4_7_FLASH)
                .context(128_000, 16_000)
                .capabilities(false, true, true, false)
                .pricing(0.1, 0.1)
                .build(),
            ModelInfoBuilder::new(MODEL_GLM_4_FLASH)
                .context(128_000, 16_000)
                .capabilities(false, true, true, false)
                .pricing(0.1, 0.1)
                .build(),
            // ── Vision series (64K context) ─────────────────────────────
            ModelInfoBuilder::new(MODEL_GLM_4_1V_THINKING_FLASH)
                .context(64_000, 8_000)
                .capabilities(true, true, true, true)
                .pricing(0.5, 0.5)
                .build(),
            ModelInfoBuilder::new(MODEL_GLM_4_6V_FLASH)
                .context(64_000, 8_000)
                .capabilities(true, true, true, false)
                .pricing(0.5, 0.5)
                .build(),
            ModelInfoBuilder::new(MODEL_GLM_4V_FLASH)
                .context(64_000, 8_000)
                .capabilities(true, true, true, false)
                .pricing(0.1, 0.1)
                .build(),
        ]
    }
}
