use crate::model::ModelInfo;

// ─── Model IDs ──────────────────────────────────────────────────────────────
// Current generation
pub const MODEL_DEEPSEEK_V4_PRO: &str = "deepseek-v4-pro";
pub const MODEL_DEEPSEEK_V4_FLASH: &str = "deepseek-v4-flash";
// Deprecated aliases — still accepted, mapped server-side to v4-flash.
pub const MODEL_DEEPSEEK_CHAT: &str = "deepseek-chat";
pub const MODEL_DEEPSEEK_REASONER: &str = "deepseek-reasoner";

pub const DEFAULT_BASE_URL: &str = "https://api.deepseek.com/anthropic";

/// Provider type key used by `ProviderConfig::provider_type`.
pub const PROVIDER_TYPE: &str = "deepseek";

pub struct DeepseekProvider;

impl DeepseekProvider {
    /// Preset model catalogue for the deepseek provider type — metadata only.
    ///
    /// `provider_id` is left nil; the caller binds a model to a provider
    /// instance when persisting it.
    pub fn preset_models() -> Vec<ModelInfo> {
        Self::model_definitions()
    }

    fn model_definitions() -> Vec<ModelInfo> {
        vec![
            // ── Current generation ───────────────────────────────────────
            // V4 Pro — flagship reasoning model.
            ModelInfo {
                model_name: MODEL_DEEPSEEK_V4_PRO.to_string(),
                provider_id: uuid::Uuid::nil(),
                context_length: 128_000,
                max_output_tokens: 32_000,
                vision_ability: false,
                supports_function_calling: true,
                supports_streaming: true,
                supports_thinking: true,
                input_token_price: 0.5,
                output_token_price: 2.0,
            },
            // V4 Flash — fast, low-cost, still supports thinking.
            ModelInfo {
                model_name: MODEL_DEEPSEEK_V4_FLASH.to_string(),
                provider_id: uuid::Uuid::nil(),
                context_length: 128_000,
                max_output_tokens: 32_000,
                vision_ability: false,
                supports_function_calling: true,
                supports_streaming: true,
                supports_thinking: true,
                input_token_price: 0.1,
                output_token_price: 0.3,
            },
            // ── Deprecated aliases (kept for backwards compatibility) ─────
            // deepseek-chat — non-thinking mode of v4-flash.
            ModelInfo {
                model_name: MODEL_DEEPSEEK_CHAT.to_string(),
                provider_id: uuid::Uuid::nil(),
                context_length: 64_000,
                max_output_tokens: 8_000,
                vision_ability: false,
                supports_function_calling: true,
                supports_streaming: true,
                supports_thinking: false,
                input_token_price: 0.1,
                output_token_price: 0.3,
            },
            // deepseek-reasoner — thinking mode of v4-flash.
            ModelInfo {
                model_name: MODEL_DEEPSEEK_REASONER.to_string(),
                provider_id: uuid::Uuid::nil(),
                context_length: 64_000,
                max_output_tokens: 8_000,
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
