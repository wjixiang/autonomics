use std::sync::Arc;

use agentik_sdk::model::model_pool::ModelPool;
use uuid::Uuid;

use crate::agent::{Agent, AgentConfig, TokenBudget};
use crate::context::ContextProvider;
use crate::error::AgentError;
use agentik_sdk::types::messages::Message;
use crate::storage::AgentSnapshotStorage;
use crate::{lifecycle::AgentLifecycle, memory::Memory, tools::Toolset};
use crate::tools::{SkillActivationState, ToolRegistration};

use agentik_skill::types::Skill;
use agentik_skill_client::SkillRegistryClient;

pub struct AgentBuilder {
    model_pool: Option<Arc<ModelPool>>,
    initial_messages: Vec<Message>,
    context_provider: Option<Arc<dyn ContextProvider>>,
    config: AgentConfig,
    storage: Option<Arc<dyn AgentSnapshotStorage>>,
    tools: Vec<ToolRegistration>,
    system_prompt_section: Option<String>,
    system_prompt_identity: Option<String>,
    event_tx: Option<tokio::sync::mpsc::UnboundedSender<agentik_sdk::types::AgentUiEvent>>,
    skill_client: Option<Arc<tokio::sync::Mutex<SkillRegistryClient>>>,
    skill_activation_state: Option<SkillActivationState>,
    /// Pre-built toolset — when set, skips automatic tool registration.
    prebuilt_toolset: Option<Toolset>,
    /// Pre-built skill path — when set, skips skill_client auto-loading.
    prebuilt_skill_path: Option<Vec<Skill>>,
}

impl Clone for AgentBuilder {
    fn clone(&self) -> Self {
        Self {
            model_pool: self.model_pool.clone(),
            initial_messages: self.initial_messages.clone(),
            context_provider: self.context_provider.clone(),
            config: self.config.clone(),
            storage: self.storage.clone(),
            tools: Vec::new(), // ToolRegistration is not Clone; re-register if needed
            system_prompt_section: self.system_prompt_section.clone(),
            system_prompt_identity: self.system_prompt_identity.clone(),
            event_tx: self.event_tx.clone(),
            skill_client: self.skill_client.clone(),
            skill_activation_state: self.skill_activation_state.clone(),
            prebuilt_toolset: None,
            prebuilt_skill_path: self.prebuilt_skill_path.clone(),
        }
    }
}

impl AgentBuilder {
    pub fn new() -> Self {
        Self {
            model_pool: None,
            initial_messages: Vec::new(),
            context_provider: None,
            config: AgentConfig::default(),
            storage: None,
            tools: Vec::new(),
            system_prompt_section: None,
            system_prompt_identity: None,
            event_tx: None,
            skill_client: None,
            skill_activation_state: None,
            prebuilt_toolset: None,
            prebuilt_skill_path: None,
        }
    }

    pub fn with_config(mut self, config: AgentConfig) -> Self {
        self.config = config;
        self
    }

    pub fn with_model_pool(mut self, pool: Arc<ModelPool>) -> Self {
        self.model_pool = Some(pool);
        self
    }

    /// Set initial messages to seed the agent's memory at build time.
    pub fn with_initial_messages(mut self, messages: Vec<Message>) -> Self {
        self.initial_messages = messages;
        self
    }

    /// Set an optional context provider for dynamic context injection.
    pub fn with_context_provider(mut self, provider: Arc<dyn ContextProvider>) -> Self {
        self.context_provider = Some(provider);
        self
    }

    pub fn with_storage(mut self, storage: Arc<dyn AgentSnapshotStorage>) -> Self {
        self.storage = Some(storage);
        self
    }

    /// Register additional tools on the agent (beyond the built-in lifecycle tools).
    pub fn with_tools(mut self, tools: Vec<ToolRegistration>) -> Self {
        self.tools = tools;
        self
    }

    /// Set a static extra section for the system prompt.
    pub fn with_system_prompt_section(mut self, section: impl Into<String>) -> Self {
        self.system_prompt_section = Some(section.into());
        self
    }

    /// Set the agent identity line for the system prompt (e.g. "You are a biomedical research assistant.").
    pub fn with_system_prompt_identity(mut self, identity: impl Into<String>) -> Self {
        self.system_prompt_identity = Some(identity.into());
        self
    }

    /// Wire an event channel for streaming `AgentUiEvent`s to external observers (e.g. a TUI).
    pub fn with_event_tx(
        mut self,
        tx: tokio::sync::mpsc::UnboundedSender<agentik_sdk::types::AgentUiEvent>,
    ) -> Self {
        self.event_tx = Some(tx);
        self
    }

    /// Connect to a skill registry and register the `activate_skill` tool.
    ///
    /// When set, the agent gains an `activate_skill` tool that calls the
    /// remote registry to fetch skill definitions and activate them.
    pub fn with_skill_client(
        mut self,
        client: Arc<tokio::sync::Mutex<SkillRegistryClient>>,
    ) -> Self {
        let activation_state = SkillActivationState::default();
        self.tools
            .push(crate::tools::skill_registration(client.clone(), activation_state.clone()));
        self.skill_client = Some(client);
        self.skill_activation_state = Some(activation_state);
        self
    }

    /// Provide a pre-built toolset. When set, the builder skips automatic
    /// tool registration (lifecycle + `with_tools`) and uses this toolset
    /// directly.
    ///
    /// Used by `AgentBlueprint::build_agent()` which constructs the toolset from
    /// the skill tree's `allowed_tools` union.
    pub fn with_toolset(mut self, toolset: Toolset) -> Self {
        self.prebuilt_toolset = Some(toolset);
        self
    }

    /// Provide a pre-built skill path. When set, the builder skips
    /// `skill_client` auto-loading and uses this path directly.
    ///
    /// The last element is the currently active (leaf) skill.
    pub fn with_skill_path(mut self, path: Vec<Skill>) -> Self {
        self.prebuilt_skill_path = Some(path);
        self
    }

    /// Set a skill activation state for runtime skill switching.
    ///
    /// Used by `AgentBlueprint::build_agent()` when a skill client is
    /// present — the same state is shared with the `activate_skill` tool
    /// so that `handle_effect()` can pick up pending activations.
    pub fn with_skill_activation_state(mut self, state: Option<SkillActivationState>) -> Self {
        self.skill_activation_state = state;
        self
    }

    pub async fn build(self) -> Result<Agent, AgentError> {
        let model_pool = self
            .model_pool
            .ok_or_else(|| AgentError::MissingConfig("model_pool".to_string()))?;

        // Build toolset: use prebuilt if provided, otherwise auto-register.
        let toolset = if let Some(toolset) = self.prebuilt_toolset {
            toolset
        } else {
            let mut toolset = Toolset::default();
            toolset.register_all(crate::tools::lifecycle_registrations())?;
            toolset.register_all(self.tools)?;
            toolset
        };

        // Build skill path: use prebuilt if provided, otherwise fetch from skill_client.
        let (active_skill_path, system_prompt_section) =
            if let Some(path) = self.prebuilt_skill_path {
                // Pre-built path: extract root body for system prompt if available.
                let section_from_skill = path.first().map(|s| s.body.clone())
                    .filter(|b| !b.is_empty());
                (path, section_from_skill.or(self.system_prompt_section))
            } else if let Some(ref client) = self.skill_client {
                let mut client = client.lock().await;
                match client.get_skill_tree().await {
                    Ok(root) => {
                        tracing::info!(
                            skill_name = %root.metadata.name,
                            "root skill loaded, initializing skill path"
                        );
                        (vec![root.clone()], Some(root.body))
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "failed to fetch root skill, starting without skill tree");
                        (Vec::new(), self.system_prompt_section)
                    }
                }
            } else {
                (Vec::new(), self.system_prompt_section)
            };

        // Seed memory with initial messages
        let mut memory = Memory::new();
        for msg in self.initial_messages {
            let _ = memory.remember(msg);
        }

        Ok(Agent {
            id: Uuid::new_v4(),
            model_pool,
            memory,
            toolset,
            lifecycle: AgentLifecycle::new(),
            config: self.config,
            storage: self.storage,
            token_budget: TokenBudget::default(),
            context_provider: self.context_provider,
            system_prompt_section,
            system_prompt_identity: self.system_prompt_identity,
            event_tx: self.event_tx,
            current_model_name: None,
            active_skill_path,
            skill_activation_state: self.skill_activation_state,
        })
    }
}

impl Default for AgentBuilder {
    fn default() -> Self {
        Self::new()
    }
}
