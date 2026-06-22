//! Phloem Server — Web backend for the agentik agent framework.
//!
//! Named after phloem, the vascular tissue that transports nutrients
//! and signals between organs in plants — this server transports data,
//! events, and configuration between the agent runtime and the web UI.

mod config;
mod error;
mod middleware;
mod routes;
mod services;
mod sse;
mod state;

use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::info;

use state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "phloem_server=debug,tower_http=debug".into()),
        )
        .init();

    // Config path: PHLOEM_CONFIG env var or ./phloem.json
    let config_path = std::env::var("PHLOEM_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("phloem.json"));

    let app_config = config::AppConfig::load(&config_path)?;
    let server_config = app_config.server_config();
    let state = AppState::new(app_config, config_path);

    // Create the single default agent at startup.
    // Use the stable UUID from config (if previously persisted) so memory
    // can be restored from the latest snapshot.
    let default_id = {
        let config = state.config.read().await;
        config.default_agent_id
    };
    let handle = services::agent_manager::get_or_create_agent(
        &state,
        default_id,
        Some("You are a helpful AI assistant."),
    )
    .await
    .expect("failed to create default agent");
    *state.default_agent_id.write().await = Some(handle.id);

    // Persist the default agent UUID to config (no-op if already saved).
    services::agent_manager::persist_default_agent_id(&state, handle.id)
        .await
        .expect("failed to persist default agent id");

    info!(agent_id = %handle.id, "default agent created at startup");

    let app = Router::new()
        .merge(routes::create_router())
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(Arc::new(state));

    let addr = format!("{}:{}", server_config.host, server_config.port);
    info!("Phloem server starting on {addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
