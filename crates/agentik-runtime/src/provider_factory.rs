//! Provider factory: maps declarative [`ProviderConfig`] strings to concrete
//! [`Model`](agentik_sdk::model::Model) instances.
//!
//! This module is the runtime-internal analogue of dendrite's
//! `settings.rs::get_model_for_provider_with_creds`.  It covers all five
//! SDK providers (`mimo`, `minimax`, `sensenova`, `deepseek`, `zai`).

use thiserror::Error;

use agentik_sdk::model::Model;
use agentik_sdk::provider::LlmProvider;

use crate::model_config::ProviderConfig;

// ── Error ───────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum ProviderBuildError {
    #[error("unknown provider type: '{0}'")]
    UnknownType(String),
    #[error("model '{model}' not found for provider '{provider}'")]
    ModelNotFound { provider: String, model: String },
    #[error("provider build failed for '{provider_type}': {reason}")]
    BuildFailed { provider_type: String, reason: String },
}

// ── ProviderType enum ───────────────────────────────────────────

/// Known built-in provider types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderType {
    Mimo,
    Minimax,
    Sensenova,
    Deepseek,
    Zai,
}

impl ProviderType {
    /// Parse a provider-type string (case-insensitive).
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "mimo" => Some(Self::Mimo),
            "minimax" => Some(Self::Minimax),
            "sensenova" => Some(Self::Sensenova),
            "deepseek" => Some(Self::Deepseek),
            "zai" => Some(Self::Zai),
            _ => None,
        }
    }
}

/// All known provider-type identifiers the frontend may use.
pub fn builtin_provider_types() -> &'static [&'static str] {
    &["mimo", "minimax", "sensenova", "deepseek", "zai"]
}

/// Default model list for a provider type (used as a starting point when
/// the user hasn't fetched a live list yet).
pub fn default_models_for_type(provider_type: &str) -> Vec<String> {
    match provider_type {
        "mimo" => vec![
            "mimo-v2.5-pro".into(),
            "mimo-v2-pro".into(),
            "mimo-v2.5".into(),
            "mimo-v2-omni".into(),
            "mimo-v2-flash".into(),
        ],
        "minimax" => vec!["MiniMax-M2.7".into()],
        "sensenova" => vec!["sensenova-6.7-flash-lite".into()],
        "deepseek" => vec!["deepseek-v4-flash".into()],
        "zai" => vec![],
        _ => Vec::new(),
    }
}

/// Default base URL for a provider type (empty if the SDK's built-in
/// default should be used).
pub fn default_base_url_for_type(provider_type: &str) -> &'static str {
    match provider_type {
        "minimax" => "https://api.minimaxi.com/anthropic",
        "sensenova" => "https://token.sensenova.cn",
        _ => "",
    }
}

// ── Mimo endpoint helper ─────────────────────────────────────────

use agentik_sdk::provider::mimo::MimoEndpoint;
use agentik_sdk::provider::mimo::TokenPlanRegion;

/// Translate a persisted mimo base URL into a [`MimoEndpoint`].
/// Mirrors dendrite's `mimo_endpoint_from_url`.
fn mimo_endpoint_from_url(url: &str) -> Option<MimoEndpoint> {
    match url {
        "" => None,
        "https://api.xiaomimimo.com/anthropic" => Some(MimoEndpoint::Api),
        "https://token-plan-cn.xiaomimimo.com/anthropic" => {
            Some(MimoEndpoint::TokenPlan(TokenPlanRegion::China))
        }
        "https://token-plan-eur.xiaomimimo.com/anthropic" => {
            Some(MimoEndpoint::TokenPlan(TokenPlanRegion::Eur))
        }
        "https://token-plan-sgp.xiaomimimo.com/anthropic" => {
            Some(MimoEndpoint::TokenPlan(TokenPlanRegion::Sgp))
        }
        _ => None, // Custom / unknown — fall back to SDK default.
    }
}

// ── Zai endpoint helper ──────────────────────────────────────────

// NOTE: ZaiEndpoint may not have a TokenPlan-style enum.  If the SDK only
// accepts `Option<String>` or a simple endpoint, we pass `None` for unknown
// URLs and let the SDK use its default.  Adjust when implementing if the
// actual Zai constructor differs.

// ── Core factory ──────────────────────────────────────────────────

/// Build a [`Model`] from a provider configuration + model name.
///
/// Matches `provider_type` to the corresponding SDK provider, instantiates
/// it with the configured credentials, and resolves the model.
pub fn build_model(cfg: &ProviderConfig, model: &str) -> Result<Model, ProviderBuildError> {
    match cfg.provider_type.to_ascii_lowercase().as_str() {
        "mimo" => {
            let p = agentik_sdk::provider::mimo::MimoProvider::new(
                mimo_endpoint_from_url(&cfg.base_url),
                cfg.api_key.clone(),
            );
            p.get_model(model).map_err(|_| ProviderBuildError::ModelNotFound {
                provider: cfg.display_name.clone(),
                model: model.to_string(),
            })
        }
        "minimax" => {
            let mut p = agentik_sdk::provider::minimax::MinimaxProvider::new(
                cfg.base_url.clone(),
                cfg.api_key.clone(),
            );
            register_custom_models(&mut p, &cfg.models);
            p.get_model(model).map_err(|_| ProviderBuildError::ModelNotFound {
                provider: cfg.display_name.clone(),
                model: model.to_string(),
            })
        }
        "sensenova" => {
            let mut p = agentik_sdk::provider::sensenova::SensenovaProvider::new(
                opt_base_url(&cfg.base_url),
                cfg.api_key.clone(),
            );
            register_custom_models(&mut p, &cfg.models);
            p.get_model(model).map_err(|_| ProviderBuildError::ModelNotFound {
                provider: cfg.display_name.clone(),
                model: model.to_string(),
            })
        }
        "deepseek" => {
            let mut p = agentik_sdk::provider::deepseek::DeepseekProvider::new(
                opt_base_url(&cfg.base_url),
                cfg.api_key.clone(),
            );
            register_custom_models(&mut p, &cfg.models);
            p.get_model(model).map_err(|_| ProviderBuildError::ModelNotFound {
                provider: cfg.display_name.clone(),
                model: model.to_string(),
            })
        }
        "zai" => {
            let p = agentik_sdk::provider::zai::ZaiProvider::new(
                None, // TODO: parse zai endpoint presets if needed
                cfg.api_key.clone(),
            );
            p.get_model(model).map_err(|_| ProviderBuildError::ModelNotFound {
                provider: cfg.display_name.clone(),
                model: model.to_string(),
            })
        }
        other => Err(ProviderBuildError::UnknownType(other.to_string())),
    }
}

/// List available models for a provider by calling the SDK's `list_models`.
/// Falls back to [`default_models_for_type`] on error.
pub async fn list_provider_models(cfg: &ProviderConfig) -> Vec<String> {
    let fallback = default_models_for_type(&cfg.provider_type);
    match cfg.provider_type.to_ascii_lowercase().as_str() {
        "mimo" => {
            // mimo doesn't have async list_models; fall back.
            fallback
        }
        "minimax" => {
            let p = agentik_sdk::provider::minimax::MinimaxProvider::new(
                cfg.base_url.clone(),
                cfg.api_key.clone(),
            );
            p.list_models()
                .await
                .map(|ms| ms.into_iter().map(|m| m.model_info.model_name).collect())
                .unwrap_or(fallback)
        }
        "sensenova" => {
            let p = agentik_sdk::provider::sensenova::SensenovaProvider::new(
                opt_base_url(&cfg.base_url),
                cfg.api_key.clone(),
            );
            p.list_models()
                .await
                .map(|ms| ms.into_iter().map(|m| m.model_info.model_name).collect())
                .unwrap_or(fallback)
        }
        "deepseek" => {
            let p = agentik_sdk::provider::deepseek::DeepseekProvider::new(
                opt_base_url(&cfg.base_url),
                cfg.api_key.clone(),
            );
            p.list_models()
                .await
                .map(|ms| ms.into_iter().map(|m| m.model_info.model_name).collect())
                .unwrap_or(fallback)
        }
        "zai" => fallback,
        _ => fallback,
    }
}

// ── Helpers ──────────────────────────────────────────────────────

/// Register user-curated model names on a provider so `get_model` can
/// resolve names that aren't part of the SDK's preset set.
fn register_custom_models<P: agentik_sdk::provider::LlmProvider>(
    provider: &mut P,
    model_list: &[String],
) {
    if model_list.is_empty() {
        return;
    }
    let infos: Vec<agentik_sdk::model::ModelInfo> = model_list
        .iter()
        .map(|m| agentik_sdk::model::ModelInfo {
            model_name: m.clone(),
            provider: String::new(),
            ..Default::default()
        })
        .collect();
    provider.add_models(infos);
}

/// Convert a base_url string to `Option<String>`.  Empty → `None` (use SDK default).
fn opt_base_url(url: &str) -> Option<String> {
    if url.is_empty() {
        None
    } else {
        Some(url.to_string())
    }
}
