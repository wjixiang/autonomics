//! Settings endpoints: read and update application config.

use std::sync::Arc;

use axum::{
    Router,
    extract::State,
    routing::get,
    Json,
};

use crate::config::AppConfig;
use crate::error::AppError;
use crate::services::settings_service;
use crate::state::AppState;

/// GET /api/settings — return current application config (api_keys masked).
async fn get_settings(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let config = settings_service::load_settings(&state).await;
    let mut value = serde_json::to_value(&config).unwrap();
    // Mask api_keys in the models array
    if let Some(models) = value.get_mut("models").and_then(|m| m.as_array_mut()) {
        for model in models {
            if let Some(key) = model.get_mut("api_key").and_then(|k| k.as_str()) {
                if !key.is_empty() {
                    *model.get_mut("api_key").unwrap() = serde_json::json!("********");
                }
            }
        }
    }
    Json(value)
}

/// PUT /api/settings — replace the entire config and persist to disk.
async fn put_settings(
    State(state): State<Arc<AppState>>,
    Json(new_config): Json<AppConfig>,
) -> Result<Json<AppConfig>, AppError> {
    settings_service::save_settings(&state, new_config).await?;
    let config = settings_service::load_settings(&state).await;
    Ok(Json(config))
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/settings", get(get_settings).put(put_settings))
}
