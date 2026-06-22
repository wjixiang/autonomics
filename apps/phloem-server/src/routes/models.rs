//! Model CRUD endpoints. Models are metadata-only and reference a provider
//! instance via `provider_id`. Provider CRUD lives in [`super::providers`].

use std::sync::Arc;

use agentik_sdk::model::ModelInfo;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::AppError;
use crate::services::provider_registry;
use crate::services::settings_service;
use crate::state::AppState;

// ── DTOs ────────────────────────────────────────────────────────────────

/// Request body for creating or updating a model.
#[derive(Debug, Clone, Deserialize)]
pub struct ModelRequest {
    pub model_name: String,
    pub provider_id: Uuid,
    pub context_length: u64,
    pub max_output_tokens: u64,
    pub vision_ability: bool,
    pub supports_function_calling: bool,
    pub supports_streaming: bool,
    pub supports_thinking: bool,
    pub input_token_price: f64,
    pub output_token_price: f64,
}

/// Response body. `provider_name` is the referenced provider instance's
/// display name, resolved for convenience.
#[derive(Debug, Serialize)]
pub struct ModelResponse {
    pub model_name: String,
    pub provider_id: Uuid,
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

// ── Helpers ──────────────────────────────────────────────────────────────

fn provider_display_name(config: &crate::config::AppConfig, id: Uuid) -> String {
    config
        .providers
        .iter()
        .find(|p| p.id == id)
        .map(|p| p.name.clone())
        .unwrap_or_default()
}

fn to_response(config: &crate::config::AppConfig, info: &ModelInfo) -> ModelResponse {
    ModelResponse {
        model_name: info.model_name.clone(),
        provider_id: info.provider_id,
        provider_name: provider_display_name(config, info.provider_id),
        context_length: info.context_length,
        max_output_tokens: info.max_output_tokens,
        vision_ability: info.vision_ability,
        supports_function_calling: info.supports_function_calling,
        supports_streaming: info.supports_streaming,
        supports_thinking: info.supports_thinking,
        input_token_price: info.input_token_price,
        output_token_price: info.output_token_price,
    }
}

fn apply_request(target: &mut ModelInfo, req: ModelRequest) {
    target.model_name = req.model_name;
    target.provider_id = req.provider_id;
    target.context_length = req.context_length;
    target.max_output_tokens = req.max_output_tokens;
    target.vision_ability = req.vision_ability;
    target.supports_function_calling = req.supports_function_calling;
    target.supports_streaming = req.supports_streaming;
    target.supports_thinking = req.supports_thinking;
    target.input_token_price = req.input_token_price;
    target.output_token_price = req.output_token_price;
}

fn request_to_model(req: ModelRequest) -> ModelInfo {
    ModelInfo {
        model_name: req.model_name,
        provider_id: req.provider_id,
        context_length: req.context_length,
        max_output_tokens: req.max_output_tokens,
        vision_ability: req.vision_ability,
        supports_function_calling: req.supports_function_calling,
        supports_streaming: req.supports_streaming,
        supports_thinking: req.supports_thinking,
        input_token_price: req.input_token_price,
        output_token_price: req.output_token_price,
    }
}

/// Reject if `provider_id` doesn't reference a known provider instance.
fn require_provider(
    config: &crate::config::AppConfig,
    provider_id: Uuid,
) -> Result<(), AppError> {
    if !config.providers.iter().any(|p| p.id == provider_id) {
        return Err(AppError::BadRequest(format!(
            "unknown provider_id {provider_id}"
        )));
    }
    Ok(())
}

// ── Handlers ────────────────────────────────────────────────────────────

async fn list_models(State(state): State<Arc<AppState>>) -> Json<Vec<ModelResponse>> {
    let config = state.config.read().await;
    let models: Vec<ModelResponse> =
        config.models.iter().map(|m| to_response(&config, m)).collect();
    Json(models)
}

async fn create_model(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ModelRequest>,
) -> Result<(StatusCode, Json<ModelResponse>), AppError> {
    if req.model_name.trim().is_empty() {
        return Err(AppError::BadRequest("model_name must not be empty".into()));
    }

    let mut config = state.config.read().await.clone();
    require_provider(&config, req.provider_id)?;
    if config.models.iter().any(|m| m.model_name == req.model_name) {
        return Err(AppError::BadRequest(format!(
            "model '{}' already exists",
            req.model_name
        )));
    }

    config.models.push(request_to_model(req.clone()));
    settings_service::save_settings(&state, config).await?;

    let config = state.config.read().await;
    let created = config
        .models
        .iter()
        .find(|m| m.model_name == req.model_name)
        .map(|m| to_response(&config, m))
        .expect("model was just inserted");
    Ok((StatusCode::CREATED, Json(created)))
}

async fn update_model(
    State(state): State<Arc<AppState>>,
    Path(model_name): Path<String>,
    Json(req): Json<ModelRequest>,
) -> Result<Json<ModelResponse>, AppError> {
    let mut config = state.config.read().await.clone();
    require_provider(&config, req.provider_id)?;

    let idx = config
        .models
        .iter()
        .position(|m| m.model_name == model_name)
        .ok_or_else(|| AppError::NotFound(format!("model '{}' not found", model_name)))?;

    // If renaming, check the new name doesn't conflict
    if req.model_name != model_name
        && config
            .models
            .iter()
            .any(|m| m.model_name == req.model_name)
    {
        return Err(AppError::BadRequest(format!(
            "model '{}' already exists",
            req.model_name
        )));
    }

    apply_request(&mut config.models[idx], req.clone());
    let new_model_name = config.models[idx].model_name.clone();
    settings_service::save_settings(&state, config).await?;

    let config = state.config.read().await;
    let response = config
        .models
        .iter()
        .find(|m| m.model_name == new_model_name)
        .map(|m| to_response(&config, m))
        .expect("model still exists");
    Ok(Json(response))
}

async fn delete_model(
    State(state): State<Arc<AppState>>,
    Path(model_name): Path<String>,
) -> Result<StatusCode, AppError> {
    let mut config = state.config.read().await.clone();
    let len_before = config.models.len();
    config.models.retain(|m| m.model_name != model_name);
    if config.models.len() == len_before {
        return Err(AppError::NotFound(format!(
            "model '{}' not found",
            model_name
        )));
    }

    settings_service::save_settings(&state, config).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Provider type presets ─────────────────────────────────────────────────

async fn list_provider_types() -> Json<Vec<provider_registry::ProviderTypeMeta>> {
    Json(provider_registry::list_provider_types())
}

// ── Router ───────────────────────────────────────────────────────────────

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/api/models",
            get(list_models).post(create_model),
        )
        .route(
            "/api/models/{model_name}",
            put(update_model).delete(delete_model),
        )
        .route("/api/provider-types", get(list_provider_types))
}
