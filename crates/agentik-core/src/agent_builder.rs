use std::sync::Arc;

use agentik_sdk::model::model_pool::ModelPool;
use uuid::Uuid;

use crate::agent::{Agent, AgentConfig, TokenBudget};
use crate::context::AgentContext;
use crate::error::AgentError;
use crate::storage::AgentSnapshotStorage;
use crate::{lifecycle::AgentLifecycle, memory::Memory, tools::Toolset};
use crate::tools::{SkillActivationState, ToolRegistration};

use agentik_skill_client::SkillRegistryClient;

pub struct AgentBuilder {
    model_pool: Option<Arc<ModelPool>>,
    ctx: Option<Arc<dyn AgentContext>>,
    config: AgentConfig,
    storage: Option<Arc<dyn AgentSnapshotStorage>>,
    tools: Vec<ToolRegistration>,
    system_prompt_section: Option<String>,
    system_prompt_identity: Option<String>,
    event_tx: Option<tokio::sync::mpsc::UnboundedSender<agentik_sdk::types::AgentUiEvent>>,
    skill_client: Option<Arc<tokio::sync::Mutex<SkillRegistryClient>>>,
    skill_activation_state: Option<SkillActivationState>,
}

impl Clone for AgentBuilder {
    fn clone(&self) -> Self {
        Self {
            model_pool: self.model_pool.clone(),
            ctx: self.ctx.clone(),
            config: self.config.clone(),
            storage: self.storage.clone(),
            tools: Vec::new(), // ToolRegistration is not Clone; re-register if needed
            system_prompt_section: self.system_prompt_section.clone(),
            system_prompt_identity: self.system_prompt_identity.clone(),
            event_tx: self.event_tx.clone(),
            skill_client: self.skill_client.clone(),
            skill_activation_state: self.skill_activation_state.clone(),
        }
    }
}

impl AgentBuilder {
    pub fn new() -> Self {
        Self {
            model_pool: None,
            ctx: None,
            config: AgentConfig::default(),
            storage: None,
            tools: Vec::new(),
            system_prompt_section: None,
            system_prompt_identity: None,
            event_tx: None,
            skill_client: None,
            skill_activation_state: None,
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

    pub fn with_context(mut self, ctx: Arc<dyn AgentContext>) -> Self {
        self.ctx = Some(ctx);
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

    pub async fn build(self) -> Result<Agent, AgentError> {
        let model_pool = self
            .model_pool
            .ok_or_else(|| AgentError::MissingConfig("model_pool".to_string()))?;
        let ctx = self
            .ctx
            .ok_or_else(|| AgentError::MissingConfig("context".to_string()))?;

        let mut toolset = Toolset::default();
        toolset.register_all(crate::tools::lifecycle_registrations())?;
        toolset.register_all(self.tools)?;

        Ok(Agent {
            id: Uuid::new_v4(),
            model_pool,
            memory: Memory::new(),
            toolset,
            lifecycle: AgentLifecycle::new(),
            config: self.config,
            storage: self.storage,
            token_budget: TokenBudget::default(),
            ctx,
            last_context_version: 0,
            system_prompt_section: self.system_prompt_section,
            system_prompt_identity: self.system_prompt_identity,
            event_tx: self.event_tx,
            current_model_name: None,
            active_skill: None,
            skill_activation_state: self.skill_activation_state,
        })
    }
}

impl Default for AgentBuilder {
    fn default() -> Self {
        Self::new()
    }
}
