use std::sync::Arc;

use agentik_skill::Skill;

use crate::sqlite_store::SkillChangeNotification;
use crate::store::{SkillStore, SkillStoreResult};

/// In-process skill registry.
///
/// Wraps a [`SkillStore`] and provides lookup, filtering, and reload
/// operations that the gRPC service delegates to.
pub struct SkillRegistry {
    store: Arc<dyn SkillStore>,
}

impl SkillRegistry {
    pub fn new(store: Arc<dyn SkillStore>) -> Self {
        Self { store }
    }

    /// Get a skill by name or alias.
    pub async fn get_skill(&self, name: &str) -> SkillStoreResult<Skill> {
        self.store.get(name).await
    }

    /// List all skills, with optional filters.
    pub async fn list_skills(
        &self,
        user_invocable_only: bool,
        model_invocable_only: bool,
    ) -> SkillStoreResult<Vec<Skill>> {
        let all = self.store.load_all().await?;
        let filtered: Vec<Skill> = all
            .into_iter()
            .filter(|s| {
                if user_invocable_only && !s.metadata.user_invocable {
                    return false;
                }
                if model_invocable_only && !s.metadata.model_invocable {
                    return false;
                }
                true
            })
            .collect();
        Ok(filtered)
    }

    /// Reload a skill by name.
    pub async fn reload_skill(&self, name: &str) -> SkillStoreResult<Option<Skill>> {
        self.store.reload(name).await
    }

    /// Get the root skill (with auto-generated children summary in body).
    pub async fn get_root_skill(&self) -> SkillStoreResult<Skill> {
        self.store.get_root_skill().await
    }
}
