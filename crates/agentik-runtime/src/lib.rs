//! Multi-agent runtime layer for `agentik`.
//!
//! This crate is the **sole interface** between the agentik project and the
//! frontend (`agentik-tui`).  It exposes:
//!
//! - A shared [`ModelPool`] singleton via [`PoolOwner`](pool::PoolOwner),
//!   configured from declarative [`ModelConfig`] types.
//! - An [`AgentRegistry`] of named agent kinds (registered by host code),
//!   so the frontend can spawn agents by name without touching
//!   `agentik-core` / `agentik-sdk` types.
//! - [`ProcessManager`] for lifecycle control (start / stop / restart /
//!   reconfigure-pool) and event observation.

pub mod model_config;
pub mod pool;
pub mod process;
pub mod provider_factory;
pub mod registry;

// ── Re-exports — frontend-facing surface ─────────────────────

// Process manager (the main public API).
pub use process::{ProcessError, ProcessEvent, ProcessExitStatus, ProcessManager};

// Declarative model configuration (pure serde data — no core/sdk types).
pub use model_config::{ModelConfig, PoolEntry, ProviderConfig};

// Model-pool singleton owner.
pub use pool::{PoolBuildError, PoolOwner};

// Agent registry and spawn options.
pub use registry::{
    AgentKindError, AgentKindFactory, AgentRegistry, AgentSpawnOpts,
};

// Provider factory helpers (for hosts that need to refresh model lists).
pub use provider_factory::{
    build_model, default_base_url_for_type, default_models_for_type, builtin_provider_types,
    list_provider_models, ProviderBuildError, ProviderType,
};

// Re-export pure-data types from agentik-sdk so the frontend doesn't
// need to depend on agentik-sdk directly.
pub use agentik_sdk::types::{AgentEvent, AgentUiEvent};

