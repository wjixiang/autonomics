//! Provider type registry — exposes SDK provider presets to the API.
//!
//! This module maps SDK provider types (mimo, deepseek, zai, minimax, sensenova)
//! to their endpoint presets and model definitions, enabling the frontend to
//! offer one-click model creation from known configurations.

use agentik_sdk::model::ModelInfo;
use serde::{Deserialize, Serialize};

// ── Response types ──────────────────────────────────────────────────────

/// Metadata about a single provider type (served by `GET /api/provider-types`).
#[derive(Debug, Clone, Serialize)]
pub struct ProviderTypeMeta {
    pub type_name: String,
    pub display_name: String,
    pub auth_method: String,
    pub endpoint_presets: Vec<EndpointPreset>,
    pub models: Vec<ModelPreset>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EndpointPreset {
    pub label: String,
    pub url: String,
}

/// A model preset without connection fields (for display in the UI).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPreset {
    pub model_name: String,
    pub provider_name: String,
    pub context_length: u64,
    pub max_output_tokens: u64,
    pub vision_ability: bool,
    pub supports_function_calling: bool,
    pub supports_streaming: bool,
    pub supports_thinking: bool,
    pub input_token_price: f64,
    pub output_token_price: f64,
}

impl ModelPreset {
    /// Build a preset DTO from a metadata-only `ModelInfo`, tagging it with the
    /// provider type name (which is no longer stored on `ModelInfo` itself).
    fn from_meta(provider_type: &str, m: &ModelInfo) -> Self {
        Self {
            model_name: m.model_name.clone(),
            provider_name: provider_type.to_string(),
            context_length: m.context_length,
            max_output_tokens: m.max_output_tokens,
            vision_ability: m.vision_ability,
            supports_function_calling: m.supports_function_calling,
            supports_streaming: m.supports_streaming,
            supports_thinking: m.supports_thinking,
            input_token_price: m.input_token_price,
            output_token_price: m.output_token_price,
        }
    }
}

// ── Known provider types ────────────────────────────────────────────────

/// Returns all known provider types with their preset metadata.
pub fn list_provider_types() -> Vec<ProviderTypeMeta> {
    vec![
        build_mimo_meta(),
        build_deepseek_meta(),
        build_zai_meta(),
        build_minimax_meta(),
        build_sensenova_meta(),
    ]
}

// ── Per-provider metadata builders ───────────────────────────────────────

fn build_mimo_meta() -> ProviderTypeMeta {
    use agentik_sdk::provider::mimo::{MimoEndpoint, MimoProvider, TokenPlanRegion};

    let endpoint_presets = vec![
        EndpointPreset {
            label: "China Token Plan".into(),
            url: MimoEndpoint::TokenPlan(TokenPlanRegion::China)
                .base_url()
                .to_string(),
        },
        EndpointPreset {
            label: "Europe Token Plan".into(),
            url: MimoEndpoint::TokenPlan(TokenPlanRegion::Eur)
                .base_url()
                .to_string(),
        },
        EndpointPreset {
            label: "Singapore Token Plan".into(),
            url: MimoEndpoint::TokenPlan(TokenPlanRegion::Sgp)
                .base_url()
                .to_string(),
        },
        EndpointPreset {
            label: "API (Pay-as-you-go)".into(),
            url: MimoEndpoint::Api.base_url().to_string(),
        },
    ];

    let models = MimoProvider::preset_models()
        .iter()
        .map(|m| ModelPreset::from_meta("mimo", m))
        .collect();

    ProviderTypeMeta {
        type_name: "mimo".into(),
        display_name: "Mimo (Xiaomi)".into(),
        auth_method: "Anthropic".into(),
        endpoint_presets,
        models,
    }
}

fn build_deepseek_meta() -> ProviderTypeMeta {
    use agentik_sdk::provider::deepseek::{DeepseekProvider, DEFAULT_BASE_URL};

    let endpoint_presets = vec![EndpointPreset {
        label: "Default".into(),
        url: DEFAULT_BASE_URL.to_string(),
    }];

    let models = DeepseekProvider::preset_models()
        .iter()
        .map(|m| ModelPreset::from_meta("deepseek", m))
        .collect();

    ProviderTypeMeta {
        type_name: "deepseek".into(),
        display_name: "DeepSeek".into(),
        auth_method: "Anthropic".into(),
        endpoint_presets,
        models,
    }
}

fn build_zai_meta() -> ProviderTypeMeta {
    use agentik_sdk::provider::zai::{ZaiEndpoint, ZaiProvider};

    let endpoint_presets = vec![
        EndpointPreset {
            label: "Token Plan (Coding)".into(),
            url: ZaiEndpoint::TokenPlan.base_url().to_string(),
        },
        EndpointPreset {
            label: "Open API".into(),
            url: ZaiEndpoint::Api.base_url().to_string(),
        },
    ];

    let models = ZaiProvider::preset_models()
        .iter()
        .map(|m| ModelPreset::from_meta("zai", m))
        .collect();

    ProviderTypeMeta {
        type_name: "zai".into(),
        display_name: "ZAI (Zhipu / BigModel)".into(),
        auth_method: "Bearer".into(),
        endpoint_presets,
        models,
    }
}

fn build_minimax_meta() -> ProviderTypeMeta {
    ProviderTypeMeta {
        type_name: "minimax".into(),
        display_name: "MiniMax".into(),
        auth_method: "Bearer".into(),
        endpoint_presets: vec![],
        models: agentik_sdk::provider::minimax::MinimaxProvider::preset_models()
            .iter()
            .map(|m| ModelPreset::from_meta("minimax", m))
            .collect(),
    }
}

fn build_sensenova_meta() -> ProviderTypeMeta {
    use agentik_sdk::provider::sensenova::{SensenovaProvider, DEFAULT_BASE_URL};

    let endpoint_presets = vec![EndpointPreset {
        label: "Default".into(),
        url: DEFAULT_BASE_URL.to_string(),
    }];

    let models = SensenovaProvider::preset_models()
        .iter()
        .map(|m| ModelPreset::from_meta("sensenova", m))
        .collect();

    ProviderTypeMeta {
        type_name: "sensenova".into(),
        display_name: "SenseNova".into(),
        auth_method: "Bearer".into(),
        endpoint_presets,
        models,
    }
}
