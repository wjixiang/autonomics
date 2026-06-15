//! [`PoolOwner`] — holds the shared [`ModelPool`] singleton for all agents.
//!
//! The pool is built from a declarative [`ModelConfig`] and stored behind a
//! `RwLock`.  Re-configuration builds a fresh pool and atomically swaps the
//! `Arc<ModelPool>` so existing agents continue to hold a reference to the old
//! pool until they are rebuilt.

use std::sync::Arc;

use thiserror::Error;

use agentik_sdk::model::model_pool::ModelPool;

use crate::model_config::ModelConfig;
use crate::provider_factory;

/// Errors produced by [`PoolOwner`].
#[derive(Debug, Error)]
pub enum PoolBuildError {
    #[error("no pool entries configured")]
    Empty,
    #[error("provider '{0}' referenced by a pool entry not found")]
    UnknownProvider(String),
    #[error("model '{model}' could not be built for provider '{provider}'")]
    ModelBuildFailed { provider: String, model: String },
}

/// Owns the single shared [`ModelPool`] used by all agents.
///
/// The pool is built from a [`ModelConfig`] via [`configure`](Self::configure)
/// or [`reconfigure`](Self::reconfigure).  Because [`ModelPool::add_model`]
/// requires `&mut self`, the pool cannot be mutated in-place once shared.
/// Instead, re-configuration builds a completely new pool and swaps the `Arc`.
pub struct PoolOwner {
    current: tokio::sync::RwLock<Option<Arc<ModelPool>>>,
}

impl Default for PoolOwner {
    fn default() -> Self {
        Self::new()
    }
}

impl PoolOwner {
    /// Create a new, unconfigured pool owner.
    pub fn new() -> Self {
        Self {
            current: tokio::sync::RwLock::new(None),
        }
    }

    /// Build a [`ModelPool`] from the given config and install it.
    ///
    /// Returns the newly created `Arc<ModelPool>`.
    pub async fn configure(
        &self,
        cfg: &ModelConfig,
    ) -> Result<Arc<ModelPool>, PoolBuildError> {
        let pool = build_pool(cfg)?;
        let arc = Arc::new(pool);
        *self.current.write().await = Some(arc.clone());
        Ok(arc)
    }

    /// Build a new pool from the config and atomically swap it in,
    /// replacing the previous pool.  Returns the new `Arc<ModelPool>`.
    ///
    /// **Note:** existing agents still reference the *old* `Arc<ModelPool>`.
    /// The caller (typically `ProcessManager::reconfigure_pool`) is
    /// responsible for rebuilding agents if they should use the new pool.
    pub async fn reconfigure(
        &self,
        cfg: &ModelConfig,
    ) -> Result<Arc<ModelPool>, PoolBuildError> {
        self.configure(cfg).await
    }

    /// Snapshot the current pool.  `None` until the first successful
    /// [`configure`](Self::configure).
    pub async fn current(&self) -> Option<Arc<ModelPool>> {
        self.current.read().await.clone()
    }

    /// Return the model names in the current pool, or an empty vec if
    /// unconfigured.
    pub async fn model_names(&self) -> Vec<String> {
        match self.current.read().await.as_ref() {
            Some(pool) => pool.model_names(),
            None => Vec::new(),
        }
    }
}

/// Internal: build a [`ModelPool`] from a [`ModelConfig`].
///
/// Iterates over `cfg.pool`, matches each `PoolEntry` to its
/// `ProviderConfig`, calls [`provider_factory::build_model`], and adds
/// the result to the pool.  Mirrors dendrite's `build_pool_from_entries`.
fn build_pool(cfg: &ModelConfig) -> Result<ModelPool, PoolBuildError> {
    let mut pool = ModelPool::new();
    for entry in &cfg.pool {
        let prov = cfg
            .providers
            .iter()
            .find(|p| p.id == entry.provider_id)
            .ok_or_else(|| PoolBuildError::UnknownProvider(entry.provider_id.clone()))?;
        let model = provider_factory::build_model(prov, &entry.model).map_err(|_| {
            PoolBuildError::ModelBuildFailed {
                provider: prov.display_name.clone(),
                model: entry.model.clone(),
            }
        })?;
        pool.add_model(model);
    }
    if pool.model_names().is_empty() {
        return Err(PoolBuildError::Empty);
    }
    Ok(pool)
}
