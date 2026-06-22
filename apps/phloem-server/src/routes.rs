//! Route modules and top-level router composition.

pub mod agents;
mod chat;
mod models;
mod providers;
mod settings;

use std::sync::Arc;

use axum::Router;

use crate::state::AppState;

/// Build the complete application router.
pub fn create_router() -> Router<Arc<AppState>> {
    Router::new()
        .merge(agents::routes())
        .merge(chat::routes())
        .merge(models::routes())
        .merge(providers::routes())
        .merge(settings::routes())
}
