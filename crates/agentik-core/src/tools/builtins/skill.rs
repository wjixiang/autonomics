use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use agentik_sdk::types::tools::{ToolResult, ToolResultContent};

use agentik_skill::Skill;
use agentik_skill_client::SkillRegistryClient;

use crate::tools::{ToolError, ToolFunction, ToolRegistration};

// ─── Shared activation state ──────────────────────────────────
//
// SkillToolImpl stores the fetched skill here; Agent::handle_effect
// reads it to set `active_skill`. This avoids embedding skill data in
// ToolEffect (which would need full Skill serialization).

/// Shared state between SkillToolImpl and Agent for skill activation.
#[derive(Clone, Default)]
pub struct SkillActivationState {
    inner: Arc<tokio::sync::Mutex<Option<Skill>>>,
}

impl SkillActivationState {
    /// Take the pending activated skill (if any).
    pub async fn take(&self) -> Option<Skill> {
        self.inner.lock().await.take()
    }

    /// Store an activated skill for pickup by Agent::handle_effect.
    pub(crate) async fn store(&self, skill: Skill) {
        *self.inner.lock().await = Some(skill);
    }
}

// ─── SkillToolImpl ────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize, agentik_proc::ToolInput)]
#[tool(
    name = "activate_skill",
    description = "Activate a skill to enter a multi-turn workflow. \
                  Once activated, only the tools listed in the skill's policy are available. \
                  The skill's prompt body and references are injected into context. \
                  Activation persists across subsequent turns until the task completes."
)]
pub struct ActivateSkillInput {
    #[desc = "Name or alias of the skill to activate"]
    pub name: String,
}

pub struct SkillToolImpl {
    client: Arc<tokio::sync::Mutex<SkillRegistryClient>>,
    activation_state: SkillActivationState,
}

impl SkillToolImpl {
    pub fn new(
        client: Arc<tokio::sync::Mutex<SkillRegistryClient>>,
        activation_state: SkillActivationState,
    ) -> Self {
        Self {
            client,
            activation_state,
        }
    }
}

#[async_trait]
impl ToolFunction for SkillToolImpl {
    type Input = ActivateSkillInput;

    async fn execute(
        &self,
        input: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        let typed: Self::Input =
            serde_json::from_value(input)?;

        let mut client = self.client.lock().await;

        let skill = client.get_skill(&typed.name).await.map_err(|e| {
            ToolError::ExecutionFailed {
                source: Box::new(e),
            }
        })?;

        let mut content = format!(
            "Skill '{}' activated.\n\n{}\n",
            skill.metadata.name, skill.body
        );

        if !skill.references.is_empty() {
            content.push_str("\n## References\n");
            for ref_file in &skill.references {
                content.push_str(&format!("\n### {}\n{}", ref_file.name, ref_file.content));
            }
        }

        // Store the fetched skill for Agent::handle_effect to pick up.
        self.activation_state.store(skill).await;

        Ok(ToolResult {
            tool_use_id: "activate_skill".to_string(),
            content: ToolResultContent::Text(content),
            is_error: None,
        })
    }

    fn timeout_seconds(&self) -> u64 {
        10
    }
}

/// Create a tool registration for the skill activation tool.
pub fn skill_registration(
    client: Arc<tokio::sync::Mutex<SkillRegistryClient>>,
    activation_state: SkillActivationState,
) -> ToolRegistration {
    ToolRegistration::from(SkillToolImpl::new(client, activation_state))
}
