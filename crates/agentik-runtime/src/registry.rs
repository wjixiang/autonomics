//! Agent registry — named agent "kinds" that bundle context, tools, and prompts.
//!
//! A **host binary** (which may depend on `agentik-core`) implements
//! [`AgentKindFactory`] and registers concrete kinds via [`AgentRegistry`].
//! The frontend (e.g. `agentik-tui`) only ever references kinds by name and
//! never sees `AgentContext`, `ToolRegistration`, or any `agentik-core` type.

use std::collections::HashMap;
use std::sync::Arc;

use agentik_sdk::types::messages::ContentBlock;

// Re-exported from core so the host can use them, but the frontend-facing
// API surface (AgentSpawnOpts) only contains pure data.
use agentik_core::context::AgentContext;
use agentik_core::tools::ToolRegistration;

// ── Error ───────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum AgentKindError {
    #[error("agent kind '{0}' not registered")]
    NotFound(String),
    #[error("kind '{kind}' failed to build: {reason}")]
    BuildFailed { kind: String, reason: String },
}

// ── Spawn options (frontend-facing, pure data) ───────────────────

/// Options the frontend supplies when spawning an agent by kind.
///
/// Contains only serialisable plain data — no `agentik-core` types.
#[derive(Default, Clone, Debug)]
pub struct AgentSpawnOpts {
    /// Override the system-prompt identity line
    /// (e.g. "You are a biomedical research assistant.").
    pub system_prompt_identity: Option<String>,

    /// Override the system-prompt section (task-specific instructions).
    pub system_prompt_section: Option<String>,

    /// Optional initial user message injected right after spawn.
    pub initial_message: Option<Vec<ContentBlock>>,
}

// ── Factory trait ───────────────────────────────────────────────

/// A named kind of agent.  Implemented by host code that depends on
/// `agentik-core`; the runtime calls these methods internally when
/// building or rebuilding an agent.
///
/// Using a trait object (rather than closures or an enum) keeps business
/// logic out of the runtime while giving a uniform, re-invokable rebuild
/// surface — which is what fixes the "restart drops tools" bug (the
/// factory's `build_tools` is called fresh on every rebuild).
#[async_trait::async_trait]
pub trait AgentKindFactory: Send + Sync {
    /// Unique machine name (e.g. `"compose"`, `"knowledge"`).
    fn name(&self) -> &str;

    /// Human-readable label for UI display.
    fn display_name(&self) -> &str;

    /// Build a fresh per-agent context.  Called on every spawn / rebuild.
    async fn build_context(&self) -> Result<Arc<dyn AgentContext>, AgentKindError>;

    /// Build the tool set for this kind.  Called fresh on every spawn /
    /// rebuild so restart and reconfigure don't silently drop tools.
    fn build_tools(&self) -> Vec<ToolRegistration>;

    /// Optional default prompt identity if the frontend doesn't override.
    fn default_identity(&self) -> Option<&str> {
        None
    }
}

// ── Registry ─────────────────────────────────────────────────────

/// Thread-safe registry of named agent kinds.
///
/// The host registers [`AgentKindFactory`] implementations at startup.
/// The runtime looks up kinds by name when the frontend calls
/// [`spawn_by_kind`](crate::ProcessManager::spawn_by_kind).
#[derive(Default)]
pub struct AgentRegistry {
    kinds: std::sync::RwLock<HashMap<String, Arc<dyn AgentKindFactory>>>,
}

impl AgentRegistry {
    /// Create a new, empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an agent kind.  Replaces any existing kind with the same name.
    pub fn register(&self, factory: Arc<dyn AgentKindFactory>) {
        let name = factory.name().to_string();
        self.kinds.write().unwrap().insert(name, factory);
    }

    /// Remove a registered kind by name.
    pub fn unregister(&self, name: &str) {
        self.kinds.write().unwrap().remove(name);
    }

    /// List all registered kind names.
    pub fn list(&self) -> Vec<String> {
        self.kinds.read().unwrap().keys().cloned().collect()
    }

    /// Look up a kind by name.
    pub fn get(&self, name: &str) -> Option<Arc<dyn AgentKindFactory>> {
        self.kinds.read().unwrap().get(name).cloned()
    }
}
