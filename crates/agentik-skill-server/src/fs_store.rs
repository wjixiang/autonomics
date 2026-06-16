use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use agentik_skill::{load_skills_from_dirs, reload_skill, Skill};
use async_trait::async_trait;
use tokio::sync::{broadcast, RwLock};

use crate::store::{SkillStore, SkillStoreError, SkillStoreResult};

/// Change notification emitted when a skill is added, modified, or removed.
#[derive(Debug, Clone)]
pub struct SkillChangeNotification {
    pub change_type: SkillChangeType,
    pub skill_name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SkillChangeType {
    Added,
    Modified,
    Removed,
}

/// Filesystem-backed skill store.
///
/// Loads skills from a set of directories on disk. Supports hot-reload
/// via `broadcast::Sender` change notifications.
pub struct FilesystemSkillStore {
    skill_dirs: Vec<PathBuf>,
    /// name/alias -> Skill
    skills: Arc<RwLock<HashMap<String, Skill>>>,
    change_tx: broadcast::Sender<SkillChangeNotification>,
}

impl FilesystemSkillStore {
    /// Create a new filesystem store and load all skills from `skill_dirs`.
    pub async fn new(skill_dirs: Vec<PathBuf>) -> SkillStoreResult<Self> {
        let skills = load_skills_from_dirs(&skill_dirs);

        let mut map = HashMap::new();
        for skill in &skills {
            map.insert(skill.metadata.name.clone(), skill.clone());
            for alias in &skill.metadata.aliases {
                map.insert(alias.clone(), skill.clone());
            }
        }

        let (change_tx, _) = broadcast::channel(64);

        Ok(Self {
            skill_dirs,
            skills: Arc::new(RwLock::new(map)),
            change_tx,
        })
    }

    /// Subscribe to skill change notifications.
    pub fn subscribe(&self) -> broadcast::Receiver<SkillChangeNotification> {
        self.change_tx.subscribe()
    }

    /// Notify the store that a skill has been reloaded.
    fn notify(&self, change_type: SkillChangeType, name: String) {
        let _ = self.change_tx.send(SkillChangeNotification {
            change_type,
            skill_name: name,
        });
    }
}

#[async_trait]
impl SkillStore for FilesystemSkillStore {
    async fn load_all(&self) -> SkillStoreResult<Vec<Skill>> {
        let guard = self.skills.read().await;
        // Deduplicate by skill_dir so each skill appears once
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        for (_, skill) in guard.iter() {
            let dir_key = skill.skill_dir.display().to_string();
            if seen.insert(dir_key) {
                result.push(skill.clone());
            }
        }
        Ok(result)
    }

    async fn get(&self, name: &str) -> SkillStoreResult<Skill> {
        let guard = self.skills.read().await;
        guard
            .get(name)
            .cloned()
            .ok_or(SkillStoreError::NotFound {
                name: name.to_string(),
            })
    }

    async fn list_names(&self) -> SkillStoreResult<Vec<String>> {
        let guard = self.skills.read().await;
        // Return only primary names (not aliases) by checking skill_dir uniqueness
        let mut seen = HashSet::new();
        let mut names = Vec::new();
        for (_, skill) in guard.iter() {
            if seen.insert(skill.skill_dir.display().to_string()) {
                names.push(skill.metadata.name.clone());
            }
        }
        Ok(names)
    }

    async fn reload(&self, name: &str) -> SkillStoreResult<Option<Skill>> {
        let skill = self.get(name).await?;
        match reload_skill(&skill) {
            Ok(Some(new_skill)) => {
                let new_name = new_skill.metadata.name.clone();
                let mut guard = self.skills.write().await;

                // Remove old entries
                guard.remove(&skill.metadata.name);
                for alias in &skill.metadata.aliases {
                    guard.remove(alias);
                }

                // Insert new entries
                guard.insert(new_skill.metadata.name.clone(), new_skill.clone());
                for alias in &new_skill.metadata.aliases {
                    guard.insert(alias.clone(), new_skill.clone());
                }

                drop(guard);
                self.notify(SkillChangeType::Modified, new_name);
                Ok(Some(new_skill))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(SkillStoreError::Load(e)),
        }
    }

    async fn watch_dirs(&self) -> Vec<PathBuf> {
        self.skill_dirs.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn make_skill_dir(tmp: &Path, name: &str, desc: &str, tools: &[&str]) -> PathBuf {
        let dir = tmp.join(name);
        std::fs::create_dir_all(&dir).unwrap();

        let mut frontmatter = format!(
            "---\nname: {name}\ndescription: \"{desc}\"\n"
        );
        if !tools.is_empty() {
            frontmatter.push_str("allowed_tools:\n");
            for t in tools {
                frontmatter.push_str(&format!("  - {t}\n"));
            }
        }
        frontmatter.push_str("---\nDo the thing.\n");

        std::fs::write(dir.join("SKILL.md"), frontmatter).unwrap();
        dir
    }

    #[tokio::test]
    async fn test_load_and_get() {
        let tmp = tempfile::tempdir().unwrap();
        make_skill_dir(tmp.path(), "commit", "Commit changes", &["bash", "read"]);
        make_skill_dir(tmp.path(), "review", "Review code", &["bash", "grep", "glob"]);

        let store = FilesystemSkillStore::new(vec![tmp.path().to_path_buf()])
            .await
            .unwrap();

        let all = store.load_all().await.unwrap();
        assert_eq!(all.len(), 2);

        let commit = store.get("commit").await.unwrap();
        assert_eq!(commit.metadata.name, "commit");
        assert_eq!(commit.policy.allowed_tools.len(), 2);

        let err = store.get("nonexistent").await.unwrap_err();
        assert!(matches!(err, SkillStoreError::NotFound { .. }));
    }

    #[tokio::test]
    async fn test_list_names_no_duplicates() {
        let tmp = tempfile::tempdir().unwrap();
        make_skill_dir(tmp.path(), "test", "A test skill", &[]);

        let store = FilesystemSkillStore::new(vec![tmp.path().to_path_buf()])
            .await
            .unwrap();

        let names = store.list_names().await.unwrap();
        assert_eq!(names, vec!["test"]);
    }

    #[tokio::test]
    async fn test_reload() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = make_skill_dir(tmp.path(), "commit", "Commit changes", &["bash", "read"]);

        let store = FilesystemSkillStore::new(vec![tmp.path().to_path_buf()])
            .await
            .unwrap();

        // Modify the SKILL.md
        let updated = format!(
            "---\nname: commit\ndescription: \"Commit with conventional messages\"\nallowed_tools:\n  - bash\n  - read\n  - write\n---\nUpdated prompt.\n"
        );
        std::fs::write(dir.join("SKILL.md"), updated).unwrap();

        let reloaded = store.reload("commit").await.unwrap();
        assert!(reloaded.is_some());
        let skill = reloaded.unwrap();
        assert_eq!(skill.metadata.description, "Commit with conventional messages");
        assert_eq!(skill.policy.allowed_tools.len(), 3);
    }
}
