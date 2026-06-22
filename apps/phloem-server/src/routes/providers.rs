//! Provider instance CRUD endpoints. A provider instance holds the connection
//! config (base URL, API key, auth) for one endpoint; models reference it.

use std::sync::Arc;

use agentik_sdk::http::auth::AuthMethod;
use agentik_sdk::model::ProviderConfig;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::AppError;
use crate::services::settings_service;
use crate::state::AppState;

// ── DTOs ────────────────────────────────────────────────────────────────

/// Request body for creating or updating a provider.
/// `api_key` is `Option` — `None` or `"********"` means keep the existing value.
#[derive(Debug, Clone, Deserialize)]
pub struct ProviderRequest {
    pub name: String,
    pub provider_type: String,
    pub base_url: String,
    pub api_key: Option<String>,
    pub auth_method: AuthMethod,
}

/// Response body — api_key is always masked.
#[derive(Debug, Serialize)]
pub struct ProviderResponse {
    pub id: Uuid,
    pub name: String,
    pub provider_type: String,
    pub base_url: String,
    pub api_key_masked: bool,
    pub auth_method: AuthMethod,
}

// ── Helpers ──────────────────────────────────────────────────────────────

fn to_response(p: &ProviderConfig) -> ProviderResponse {
    ProviderResponse {
        id: p.id,
        name: p.name.clone(),
        provider_type: p.provider_type.clone(),
        base_url: p.base_url.clone(),
        api_key_masked: !p.api_key.is_empty(),
        auth_method: p.auth_method.clone(),
    }
}

fn apply_request(target: &mut ProviderConfig, req: ProviderRequest) {
    target.name = req.name;
    target.provider_type = req.provider_type;
    target.base_url = req.base_url;
    target.auth_method = req.auth_method;
    // Only update api_key if a non-masked value is provided
    if let Some(key) = req.api_key {
        if key != "********" {
            target.api_key = key;
        }
    }
}

// ── Handlers ────────────────────────────────────────────────────────────

async fn list_providers(State(state): State<Arc<AppState>>) -> Json<Vec<ProviderResponse>> {
    let config = state.config.read().await;
    let providers: Vec<ProviderResponse> =
        config.providers.iter().map(to_response).collect();
    Json(providers)
}

async fn create_provider(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ProviderRequest>,
) -> Result<(StatusCode, Json<ProviderResponse>), AppError> {
    if req.name.trim().is_empty() {
        return Err(AppError::BadRequest("name must not be empty".into()));
    }
    if req.base_url.trim().is_empty() {
        return Err(AppError::BadRequest("base_url must not be empty".into()));
    }

    let mut config = state.config.read().await.clone();
    if config.providers.iter().any(|p| p.name == req.name) {
        return Err(AppError::BadRequest(format!(
            "provider '{}' already exists",
            req.name
        )));
    }

    let provider = ProviderConfig {
        id: Uuid::new_v4(),
        name: req.name.clone(),
        provider_type: req.provider_type.clone(),
        base_url: req.base_url.clone(),
        api_key: req.api_key.unwrap_or_default(),
        auth_method: req.auth_method.clone(),
    };
    let id = provider.id;
    config.providers.push(provider);
    settings_service::save_settings(&state, config).await?;

    let config = state.config.read().await;
    let created = config
        .providers
        .iter()
        .find(|p| p.id == id)
        .map(to_response)
        .expect("provider was just inserted");
    Ok((StatusCode::CREATED, Json(created)))
}

async fn update_provider(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(req): Json<ProviderRequest>,
) -> Result<Json<ProviderResponse>, AppError> {
    let mut config = state.config.read().await.clone();

    let idx = config
        .providers
        .iter()
        .position(|p| p.id == id)
        .ok_or_else(|| AppError::NotFound(format!("provider '{id}' not found")))?;

    // If renaming, check the new name doesn't conflict
    if req.name != config.providers[idx].name
        && config.providers.iter().any(|p| p.name == req.name)
    {
        return Err(AppError::BadRequest(format!(
            "provider '{}' already exists",
            req.name
        )));
    }

    apply_request(&mut config.providers[idx], req);
    settings_service::save_settings(&state, config).await?;

    let config = state.config.read().await;
    let response = config
        .providers
        .iter()
        .find(|p| p.id == id)
        .map(to_response)
        .expect("provider still exists");
    Ok(Json(response))
}

async fn delete_provider(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    let mut config = state.config.read().await.clone();

    // Reject deletion if any model still references this provider.
    if config.models.iter().any(|m| m.provider_id == id) {
        return Err(AppError::BadRequest(format!(
            "provider '{id}' is referenced by one or more models; remove them first"
        )));
    }

    let len_before = config.providers.len();
    config.providers.retain(|p| p.id != id);
    if config.providers.len() == len_before {
        return Err(AppError::NotFound(format!("provider '{id}' not found")));
    }

    settings_service::save_settings(&state, config).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Router ───────────────────────────────────────────────────────────────

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/api/providers",
            get(list_providers).post(create_provider),
        )
        .route(
            "/api/providers/{id}",
            axum::routing::put(update_provider).delete(delete_provider),
        )
}
