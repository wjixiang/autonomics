use std::path::PathBuf;

use agentik_skill::{Skill, SkillError};
use async_trait::async_trait;

/// Error type for skill store operations.
#[derive(Debug, thiserror::Error)]
pub enum SkillStoreError {
    #[error("skill not found: {name}")]
    NotFound { name: String },
    #[error("skill load error: {0}")]
    Load(#[from] SkillError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("watch error: {0}")]
    Watch(#[source] Box<dyn std::error::Error + Send + Sync>),
}

pub type SkillStoreResult<T> = Result<T, SkillStoreError>;

/// Abstract storage backend for skills.
///
/// Implementations provide persistence and optional change-notification.
/// The first implementation is filesystem-based; future implementations
/// could use SQLite, sled, or a remote database.
#[async_trait]
pub trait SkillStore: Send + Sync {
    /// Load all skills from the store.
    async fn load_all(&self) -> SkillStoreResult<Vec<Skill>>;

    /// Get a single skill by name (or alias).
    async fn get(&self, name: &str) -> SkillStoreResult<Skill>;

    /// List all skill names (primary names only, no aliases).
    async fn list_names(&self) -> SkillStoreResult<Vec<String>>;

    /// Reload a skill from its source.
    /// Returns `None` if the skill is unchanged.
    async fn reload(&self, name: &str) -> SkillStoreResult<Option<Skill>>;

    /// Return the directories being watched (for diagnostics).
    async fn watch_dirs(&self) -> Vec<PathBuf>;

    /// Get the root skill with auto-generated children summary in its body.
    async fn get_root_skill(&self) -> SkillStoreResult<Skill>;
}
