//! Agent registry — named agent "kinds" that bundle skill trees and tools.
//!
//! A **host binary** (which depends on `agentik-core`) constructs an [`AgentBlueprint`]
//! that bundles a skill tree and tool provider, then registers
//! it via [`AgentRegistry`]. The runtime calls [`AgentBlueprint::build_agent`] internally
//! when spawning or rebuilding an agent.

use std::collections::HashMap;
use std::sync::Arc;

use agentik_sdk::types::messages::ContentBlock;

use agentik_skill::SkillTree;
use agentik_core::tools::ToolProviderRegistry;
use agentik_skill_client::SkillRegistryClient;

// ── Error ───────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum AgentBlueprintError {
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
    /// Override the system-prompt identity line.
    pub system_prompt_identity: Option<String>,

    /// Override the system-prompt section (task-specific instructions).
    pub system_prompt_section: Option<String>,

    /// Optional initial user message injected right after spawn.
    pub initial_message: Option<Vec<ContentBlock>>,
}

// ── AgentBlueprint ────────────────────────────────────────────────────

/// A named kind of agent that bundles a skill tree and tool provider.
///
/// This is the unified construction layer that binds Agent + Toolset + SkillTree.
/// When `build_agent()` is called, it:
///
/// 1. Collects the full tool universe from the skill tree's `allowed_tools` union
/// 2. Builds a `Toolset` from the tool provider (only the tools the tree needs)
/// 3. Registers the `activate_skill` tool if a skill client is provided
/// 4. Initializes the agent with the root skill active
///
/// Example:
/// ```ignore
/// let kind = AgentBlueprint::new(
///     "coder",
///     "Generic Coder",
///     load_skill_tree_from_dirs(&["./skills"])?,
///     default_tool_provider(),
/// )
/// .with_identity("You are a helpful coding assistant.")
/// .with_skill_client(skill_client);
///
/// let agent = kind.build_agent(model_pool).await?;
/// ```
pub struct AgentBlueprint {
    pub name: String,
    pub display_name: String,
    pub skill_tree: SkillTree,
    pub tool_provider: ToolProviderRegistry,
    pub default_identity: Option<String>,
    /// Optional skill registry client for runtime skill activation.
    /// When set, `build_agent()` registers the `activate_skill` tool.
    pub skill_client: Option<Arc<tokio::sync::Mutex<SkillRegistryClient>>>,
}

impl AgentBlueprint {
    /// Create a new agent kind.
    ///
    /// # Arguments
    /// * `name` — Machine-readable identifier (e.g. "coder")
    /// * `display_name` — Human-readable label for UI
    /// * `skill_tree` — The skill tree loaded from disk
    /// * `tool_provider` — Global tool provider for resolving tool names to implementations
    pub fn new(
        name: impl Into<String>,
        display_name: impl Into<String>,
        skill_tree: SkillTree,
        tool_provider: ToolProviderRegistry,
    ) -> Self {
        Self {
            name: name.into(),
            display_name: display_name.into(),
            skill_tree,
            tool_provider,
            default_identity: None,
            skill_client: None,
        }
    }

    /// Set a default prompt identity for this kind.
    pub fn with_identity(mut self, identity: impl Into<String>) -> Self {
        self.default_identity = Some(identity.into());
        self
    }

    /// Set a skill registry client for runtime `activate_skill` support.
    pub fn with_skill_client(
        mut self,
        client: Arc<tokio::sync::Mutex<SkillRegistryClient>>,
    ) -> Self {
        self.skill_client = Some(client);
        self
    }

    /// Build a complete Agent from this kind's skill tree + tool provider.
    ///
    /// The toolset is derived from the skill tree's `allowed_tools` union,
    /// the root skill is activated, and the root skill's body becomes the
    /// system prompt section.
    pub async fn build_agent(
        &self,
        model_pool: Arc<agentik_core::model::model_pool::ModelPool>,
    ) -> Result<agentik_core::Agent, AgentBlueprintError> {
        use agentik_core::agent_builder::AgentBuilder;

        // 1. Collect tool universe from skill tree
        let tool_names: Vec<String> = self
            .skill_tree
            .collect_all_allowed_tools()
            .into_iter()
            .collect();

        // 2. Build toolset from tool provider (includes lifecycle tools)
        let mut toolset = self
            .tool_provider
            .build_toolset(&tool_names, true);

        // 3. Register activate_skill tool if a skill client is available
        let skill_activation_state = if let Some(client) = &self.skill_client {
            let state = agentik_core::tools::SkillActivationState::default();
            let skill_reg = agentik_core::tools::skill_registration(client.clone(), state.clone());
            if let Err(e) = toolset.register(skill_reg) {
                tracing::warn!(error = %e, "failed to register activate_skill tool");
            }
            Some(state)
        } else {
            None
        };

        // 4. Initialize skill path with root skill
        let skill_path = self
            .skill_tree
            .root
            .as_ref()
            .map(|node| vec![node.skill.clone()])
            .unwrap_or_default();

        // 5. Build agent via builder (prebuilt toolset + skill path skip auto-registration)
        let mut builder = AgentBuilder::new()
            .with_model_pool(model_pool)
            .with_toolset(toolset)
            .with_skill_path(skill_path)
            .with_skill_activation_state(skill_activation_state);

        if let Some(identity) = &self.default_identity {
            builder = builder.with_system_prompt_identity(identity.clone());
        }

        builder.build().await.map_err(|e| AgentBlueprintError::BuildFailed {
            kind: self.name.clone(),
            reason: e.to_string(),
        })
    }
}

// ── Registry ─────────────────────────────────────────────────────

/// Thread-safe registry of named agent kinds.
///
/// The host registers [`AgentBlueprint`] instances at startup.
/// The runtime looks up kinds by name when the frontend calls
/// [`spawn_by_kind`](crate::ProcessManager::spawn_by_kind).
#[derive(Default)]
pub struct AgentRegistry {
    kinds: std::sync::RwLock<HashMap<String, Arc<AgentBlueprint>>>,
}

impl AgentRegistry {
    /// Create a new, empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an agent kind.  Replaces any existing kind with the same name.
    pub fn register(&self, kind: Arc<AgentBlueprint>) {
        let name = kind.name.clone();
        self.kinds.write().unwrap().insert(name, kind);
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
    pub fn get(&self, name: &str) -> Option<Arc<AgentBlueprint>> {
        self.kinds.read().unwrap().get(name).cloned()
    }
}

