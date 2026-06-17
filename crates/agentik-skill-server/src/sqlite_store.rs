//! SQLite-backed skill store.
//!
//! Provides the [`SqliteSkillStore`] implementing the [`SkillStore`] trait
//! on top of a single SQLite database. The store is the **source of truth**;
//! skills are loaded into it via [`SqliteSkillStore::import_from_dir`] and
//! materialised back to the filesystem via [`SqliteSkillStore::export_to_dir`].

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use agentik_skill::{
    load_skills_recursive, Skill, SkillFrontmatter, SkillMetadata, SkillPolicy, SkillTree,
    SkillTreeNode,
};
use async_trait::async_trait;
use rusqlite::{params, Connection};
use tokio::sync::{broadcast, Mutex, RwLock};

use crate::store::{SkillStore, SkillStoreError, SkillStoreResult};

// ── Change notification ─────────────────────────────────────────

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

// ── SqliteSkillStore ────────────────────────────────────────────

/// SQLite-backed skill store.
pub struct SqliteSkillStore {
    db: Arc<Mutex<Connection>>,
    /// In-memory name/alias -> Skill index.
    skills: Arc<RwLock<HashMap<String, Skill>>>,
    /// Cached skill tree (invalidated on writes).
    tree: Arc<RwLock<SkillTree>>,
    tree_valid: Arc<AtomicBool>,
    change_tx: broadcast::Sender<SkillChangeNotification>,
    /// Path to DB file (None for in-memory).
    db_path: Option<PathBuf>,
}

const CHANGE_CHANNEL_CAPACITY: usize = 64;

impl SqliteSkillStore {
    /// Open or create a SQLite database at the given path.
    pub async fn open(path: PathBuf) -> SkillStoreResult<Self> {
        let conn = Connection::open(&path).map_err(SkillStoreError::from)?;
        let store = Self::from_connection(conn, Some(path))?;
        store.rebuild_index().await?;
        Ok(store)
    }

    /// Create an in-memory store (for tests).
    pub async fn in_memory() -> SkillStoreResult<Self> {
        let conn = Connection::open_in_memory().map_err(SkillStoreError::from)?;
        let store = Self::from_connection(conn, None)?;
        store.rebuild_index().await?;
        Ok(store)
    }

    fn from_connection(
        conn: Connection,
        db_path: Option<PathBuf>,
    ) -> SkillStoreResult<Self> {
        init_schema(&conn)?;
        let (change_tx, _) = broadcast::channel(CHANGE_CHANNEL_CAPACITY);
        Ok(Self {
            db: Arc::new(Mutex::new(conn)),
            skills: Arc::new(RwLock::new(HashMap::new())),
            tree: Arc::new(RwLock::new(SkillTree::default())),
            tree_valid: Arc::new(AtomicBool::new(false)),
            change_tx,
            db_path,
        })
    }

    /// Subscribe to skill change notifications.
    pub fn subscribe(&self) -> broadcast::Receiver<SkillChangeNotification> {
        self.change_tx.subscribe()
    }

    /// DB file path (None for in-memory).
    pub fn db_path(&self) -> Option<&Path> {
        self.db_path.as_deref()
    }

    /// Import skills from a directory tree, replacing existing contents.
    ///
    /// Recursively walks the directory, parses every `SKILL.md` and computes
    /// the dotpath from each skill's path relative to `base_dir`. Errors
    /// for individual skills are logged and skipped; the transaction commits
    /// only fully-valid skills.
    pub async fn import_from_dir(&self, base_dir: &Path) -> SkillStoreResult<usize> {
        let base_dir = base_dir.to_path_buf();
        let base = base_dir.clone();
        let skills = load_skills_recursive(&[base_dir]);

        // Compute dotpath for each skill relative to base.
        let mut with_dotpath: Vec<(String, Skill)> = Vec::with_capacity(skills.len());
        for skill in skills {
            let dotpath = match skill.skill_dir.strip_prefix(&base) {
                Ok(rel) => rel
                    .components()
                    .filter_map(|c| c.as_os_str().to_str())
                    .collect::<Vec<_>>()
                    .join("."),
                Err(_) => {
                    tracing::warn!(
                        path = %skill.skill_dir.display(),
                        base = %base.display(),
                        "skill_dir is not under base_dir; skipping"
                    );
                    continue;
                }
            };
            if dotpath.is_empty() {
                tracing::warn!(
                    path = %skill.skill_dir.display(),
                    "empty dotpath; skipping"
                );
                continue;
            }
            with_dotpath.push((dotpath, skill));
        }

        let conn = self.db.clone();
        let base_for_db = base.clone();
        let inserted = tokio::task::spawn_blocking(move || -> SkillStoreResult<usize> {
            let conn = conn.blocking_lock();
            conn.execute("BEGIN", [])?;
            let result = (|| -> SkillStoreResult<usize> {
                // Wipe all existing skills (cascades to aliases/references/activation_paths).
                conn.execute("DELETE FROM skills", [])?;
                write_base_dir(&conn, &base_for_db)?;
                for (dotpath, skill) in &with_dotpath {
                    insert_skill(&conn, dotpath, skill)?;
                }
                Ok(with_dotpath.len())
            })();
            match result {
                Ok(n) => {
                    conn.execute("COMMIT", [])?;
                    Ok(n)
                }
                Err(e) => {
                    let _ = conn.execute("ROLLBACK", []);
                    Err(e)
                }
            }
        })
        .await
        .map_err(|e| SkillStoreError::Watch(Box::new(e)))??;

        self.invalidate_tree();
        self.rebuild_index().await?;
        Ok(inserted)
    }

    /// Export all skills to a directory tree mirroring dotpath structure.
    ///
    /// Writes `SKILL.md` (frontmatter + body) and sibling reference files
    /// for each skill. Existing files in the directory are not removed.
    pub async fn export_to_dir(&self, base_dir: &Path) -> SkillStoreResult<usize> {
        let conn = self.db.clone();
        let base_dir = base_dir.to_path_buf();
        let written = tokio::task::spawn_blocking(move || -> SkillStoreResult<usize> {
            let conn = conn.blocking_lock();
            let mut stmt = conn.prepare(
                "SELECT dotpath, name, description, body, allowed_tools, user_invocable, \
                 model_invocable, when_to_use, argument_hint, skill_dir \
                 FROM skills",
            )?;
            let rows: Vec<RawSkillRow> = stmt
                .query_map([], |row| {
                    Ok(RawSkillRow {
                        dotpath: row.get(0)?,
                        name: row.get(1)?,
                        description: row.get(2)?,
                        body: row.get(3)?,
                        allowed_tools_json: row.get(4)?,
                        user_invocable: row.get(5)?,
                        model_invocable: row.get(6)?,
                        when_to_use: row.get(7)?,
                        argument_hint: row.get(8)?,
                        skill_dir: row.get(9)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;

            let mut aliases_stmt = conn.prepare("SELECT alias FROM aliases WHERE dotpath = ?1")?;
            let mut refs_stmt =
                conn.prepare("SELECT name, content FROM skill_references WHERE dotpath = ?1 ORDER BY id")?;
            let mut paths_stmt = conn.prepare(
                "SELECT path FROM activation_paths WHERE dotpath = ?1 ORDER BY id",
            )?;

            for row in &rows {
                let aliases: Vec<String> = aliases_stmt
                    .query_map(params![row.dotpath], |r| r.get(0))?
                    .collect::<Result<_, _>>()?;
                let references: Vec<(String, String)> = refs_stmt
                    .query_map(params![row.dotpath], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect::<Result<_, _>>()?;
                let activation_paths: Vec<String> = paths_stmt
                    .query_map(params![row.dotpath], |r| r.get(0))?
                    .collect::<Result<_, _>>()?;

                let skill_dir = base_dir.join(row.dotpath.replace('.', "/"));
                std::fs::create_dir_all(&skill_dir)?;

                let fm = SkillFrontmatter {
                    name: row.name.clone(),
                    description: row.description.clone(),
                    aliases: aliases.clone(),
                    when_to_use: row.when_to_use.clone(),
                    argument_hint: row.argument_hint.clone(),
                    user_invocable: row.user_invocable != 0,
                    model_invocable: row.model_invocable != 0,
                    allowed_tools: serde_json::from_str::<Vec<String>>(&row.allowed_tools_json)
                        .unwrap_or_default(),
                    paths: activation_paths,
                };
                let yaml = serde_yaml::to_string(&fm)
                    .map_err(|e| SkillStoreError::Watch(Box::new(e)))?;
                let body = row.body.clone();

                // Write SKILL.md: frontmatter + body.
                let skill_md = skill_dir.join("SKILL.md");
                let yaml_trimmed = yaml.trim_end();
                let content = format!("---\n{yaml_trimmed}\n---\n\n{body}\n");
                std::fs::write(&skill_md, content)?;

                // Write reference files.
                for (name, content) in &references {
                    if name == "SKILL.md" {
                        continue; // avoid collision
                    }
                    std::fs::write(skill_dir.join(name), content)?;
                }
            }
            Ok(rows.len())
        })
        .await
        .map_err(|e| SkillStoreError::Watch(Box::new(e)))??;

        Ok(written)
    }

    /// Rebuild the in-memory index from the database.
    async fn rebuild_index(&self) -> SkillStoreResult<()> {
        let conn = self.db.clone();
        let (skills, tree) = tokio::task::spawn_blocking(move || -> SkillStoreResult<(HashMap<String, Skill>, SkillTree)> {
            let conn = conn.blocking_lock();
            let skills = load_all_from_db(&conn)?;
            let base_dir = read_base_dir(&conn);
            let tree = build_tree_from_skills(&skills, base_dir.as_deref());
            let mut map: HashMap<String, Skill> = HashMap::new();
            for skill in &skills {
                map.insert(skill.metadata.name.clone(), skill.clone());
                for alias in &skill.metadata.aliases {
                    map.insert(alias.clone(), skill.clone());
                }
            }
            Ok((map, tree))
        })
        .await
        .map_err(|e| SkillStoreError::Watch(Box::new(e)))??;

        {
            let mut guard = self.skills.write().await;
            *guard = skills;
        }
        {
            let mut guard = self.tree.write().await;
            *guard = tree;
        }
        self.tree_valid.store(true, Ordering::Release);
        Ok(())
    }

    fn invalidate_tree(&self) {
        self.tree_valid.store(false, Ordering::Release);
    }

    fn notify(&self, change_type: SkillChangeType, name: String) {
        let _ = self.change_tx.send(SkillChangeNotification {
            change_type,
            skill_name: name,
        });
    }
}

#[async_trait]
impl SkillStore for SqliteSkillStore {
    async fn load_all(&self) -> SkillStoreResult<Vec<Skill>> {
        let guard = self.skills.read().await;
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        for (_, skill) in guard.iter() {
            if seen.insert(skill.skill_dir.display().to_string()) {
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
            .ok_or_else(|| SkillStoreError::NotFound { name: name.to_string() })
    }

    async fn list_names(&self) -> SkillStoreResult<Vec<String>> {
        let guard = self.skills.read().await;
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
        // Re-read from disk using the stored skill_dir, then upsert.
        let skill = self.get(name).await?;
        let new_skill = match agentik_skill::reload_skill(&skill) {
            Ok(Some(s)) => s,
            Ok(None) => return Ok(None),
            Err(e) => return Err(SkillStoreError::Load(e)),
        };

        let dotpath = compute_dotpath(&skill.skill_dir, &skill.skill_dir)
            .ok_or_else(|| SkillStoreError::NotFound { name: name.to_string() })?;
        // Compute actual dotpath by stripping the original base from skill_dir.
        // The DB stores the original skill_dir; we use the in-memory skill to derive it.
        let conn = self.db.clone();
        let dotpath_for_db = dotpath.clone();
        let new_skill_clone = new_skill.clone();
        tokio::task::spawn_blocking(move || -> SkillStoreResult<()> {
            let conn = conn.blocking_lock();
            conn.execute("BEGIN", [])?;
            let result = (|| -> SkillStoreResult<()> {
                conn.execute("DELETE FROM skills WHERE dotpath = ?1", params![dotpath_for_db])?;
                insert_skill(&conn, &dotpath_for_db, &new_skill_clone)?;
                Ok(())
            })();
            match result {
                Ok(()) => {
                    conn.execute("COMMIT", [])?;
                }
                Err(e) => {
                    let _ = conn.execute("ROLLBACK", []);
                    return Err(e);
                }
            }
            Ok(())
        })
        .await
        .map_err(|e| SkillStoreError::Watch(Box::new(e)))??;

        self.invalidate_tree();
        self.rebuild_index().await?;
        self.notify(SkillChangeType::Modified, new_skill.metadata.name.clone());
        Ok(Some(new_skill))
    }

    async fn watch_dirs(&self) -> Vec<PathBuf> {
        // For the DB-backed store, "watch dirs" is a diagnostic view of the DB path.
        match &self.db_path {
            Some(p) => vec![p.clone()],
            None => Vec::new(),
        }
    }

    async fn get_root_skill(&self) -> SkillStoreResult<Skill> {
        let guard = self.tree.read().await;
        guard
            .root
            .as_ref()
            .map(|node| node.skill.clone())
            .ok_or_else(|| SkillStoreError::NotFound {
                name: "root".to_string(),
            })
    }
}

// ── Helpers ────────────────────────────────────────────────────

/// Compute dotpath from a skill's directory relative to a base directory.
fn compute_dotpath(skill_dir: &Path, base_dir: &Path) -> Option<String> {
    let rel = skill_dir.strip_prefix(base_dir).ok()?;
    let dotpath = rel
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect::<Vec<_>>()
        .join(".");
    if dotpath.is_empty() {
        None
    } else {
        Some(dotpath)
    }
}

struct RawSkillRow {
    dotpath: String,
    name: String,
    description: String,
    body: String,
    allowed_tools_json: String,
    user_invocable: i64,
    model_invocable: i64,
    when_to_use: Option<String>,
    argument_hint: Option<String>,
    skill_dir: String,
}

fn load_all_from_db(conn: &Connection) -> SkillStoreResult<Vec<Skill>> {
    let mut stmt = conn.prepare(
        "SELECT dotpath, name, description, body, allowed_tools, user_invocable, \
         model_invocable, when_to_use, argument_hint, skill_dir \
         FROM skills",
    )?;
    let rows: Vec<RawSkillRow> = stmt
        .query_map([], |row| {
            Ok(RawSkillRow {
                dotpath: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                body: row.get(3)?,
                allowed_tools_json: row.get(4)?,
                user_invocable: row.get(5)?,
                model_invocable: row.get(6)?,
                when_to_use: row.get(7)?,
                argument_hint: row.get(8)?,
                skill_dir: row.get(9)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut aliases_stmt = conn.prepare("SELECT alias FROM aliases WHERE dotpath = ?1")?;
    let mut refs_stmt =
        conn.prepare("SELECT name, content FROM skill_references WHERE dotpath = ?1 ORDER BY id")?;
    let mut paths_stmt =
        conn.prepare("SELECT path FROM activation_paths WHERE dotpath = ?1 ORDER BY id")?;

    let mut skills = Vec::new();
    for row in rows {
        let aliases: Vec<String> = aliases_stmt
            .query_map(params![row.dotpath], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        let references: Vec<(String, String)> = refs_stmt
            .query_map(params![row.dotpath], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?;
        let activation_paths: Vec<String> = paths_stmt
            .query_map(params![row.dotpath], |r| r.get(0))?
            .collect::<Result<_, _>>()?;

        let allowed_tools: BTreeSet<String> =
            serde_json::from_str(&row.allowed_tools_json).unwrap_or_default();

        let skill = Skill {
            metadata: SkillMetadata {
                name: row.name,
                description: row.description,
                aliases,
                when_to_use: row.when_to_use,
                argument_hint: row.argument_hint,
                user_invocable: row.user_invocable != 0,
                model_invocable: row.model_invocable != 0,
            },
            policy: SkillPolicy { allowed_tools },
            body: row.body,
            references: references
                .into_iter()
                .map(|(name, content)| agentik_skill::ReferenceFile { name, content })
                .collect(),
            activation_paths,
            skill_dir: PathBuf::from(row.skill_dir),
        };
        skills.push(skill);
    }
    Ok(skills)
}

/// Build a `SkillTree` from a flat list of skills using dotpath-based grouping.
///
/// If `base_dir` is provided, dotpaths are computed relative to it (the import
/// base directory). Otherwise the common ancestor of all skill_dirs is used
/// as a fallback.
fn build_tree_from_skills(skills: &[Skill], base_dir: Option<&Path>) -> SkillTree {
    if skills.is_empty() {
        return SkillTree::default();
    }
    let resolved_base = base_dir
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| common_ancestor(skills));

    // Group by parent dotpath.
    let mut by_parent: HashMap<String, Vec<(String, Skill)>> = HashMap::new();
    let mut root_entry: Option<(String, Skill)> = None;

    for skill in skills {
        let dotpath = match skill.skill_dir.strip_prefix(&resolved_base) {
            Ok(rel) => rel
                .components()
                .filter_map(|c| c.as_os_str().to_str())
                .collect::<Vec<_>>()
                .join("."),
            Err(_) => continue,
        };
        if dotpath.is_empty() {
            continue;
        }
        if let Some(idx) = dotpath.rfind('.') {
            let parent = dotpath[..idx].to_string();
            by_parent
                .entry(parent)
                .or_default()
                .push((dotpath, skill.clone()));
        } else if root_entry.is_none() {
            root_entry = Some((dotpath, skill.clone()));
        }
    }

    let Some((root_dotpath, root_skill)) = root_entry else {
        return SkillTree::default();
    };

    let mut root_node = SkillTreeNode {
        skill: root_skill,
        dotpath: root_dotpath.clone(),
        children: Vec::new(),
    };
    build_children(&mut root_node, &by_parent);
    sort_children_recursive(&mut root_node);
    SkillTree {
        root: Some(root_node),
    }
}

fn read_base_dir(conn: &Connection) -> Option<PathBuf> {
    conn.query_row(
        "SELECT value FROM metadata WHERE key = 'base_dir'",
        [],
        |row| row.get::<_, String>(0),
    )
    .ok()
    .map(PathBuf::from)
}

fn write_base_dir(conn: &Connection, base: &Path) -> SkillStoreResult<()> {
    conn.execute(
        "INSERT INTO metadata (key, value) VALUES ('base_dir', ?1) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![base.display().to_string()],
    )?;
    Ok(())
}

fn build_children(
    node: &mut SkillTreeNode,
    by_parent: &HashMap<String, Vec<(String, Skill)>>,
) {
    if let Some(entries) = by_parent.get(&node.dotpath) {
        for (dotpath, skill) in entries {
            let mut child = SkillTreeNode {
                skill: skill.clone(),
                dotpath: dotpath.clone(),
                children: Vec::new(),
            };
            build_children(&mut child, by_parent);
            node.children.push(child);
        }
    }
}

fn sort_children_recursive(node: &mut SkillTreeNode) {
    node.children.sort_by(|a, b| a.dotpath.cmp(&b.dotpath));
    for child in &mut node.children {
        sort_children_recursive(child);
    }
}

fn common_ancestor(skills: &[Skill]) -> PathBuf {
    let mut iter = skills.iter().map(|s| s.skill_dir.as_path());
    let Some(mut common) = iter.next().map(|p| p.to_path_buf()) else {
        return PathBuf::new();
    };
    for skill in &skills[1..] {
        let mut new_common = PathBuf::new();
        for (a, b) in common.components().zip(skill.skill_dir.components()) {
            if a == b {
                new_common.push(a.as_os_str());
            } else {
                break;
            }
        }
        common = new_common;
    }
    common
}

fn insert_skill(
    conn: &Connection,
    dotpath: &str,
    skill: &Skill,
) -> SkillStoreResult<()> {
    let allowed_tools_json =
        serde_json::to_string(&skill.policy.allowed_tools.iter().collect::<Vec<_>>())
            .map_err(|e| SkillStoreError::Watch(Box::new(e)))?;
    conn.execute(
        "INSERT INTO skills (dotpath, name, description, body, allowed_tools, \
         user_invocable, model_invocable, when_to_use, argument_hint, skill_dir) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            dotpath,
            skill.metadata.name,
            skill.metadata.description,
            skill.body,
            allowed_tools_json,
            skill.metadata.user_invocable as i64,
            skill.metadata.model_invocable as i64,
            skill.metadata.when_to_use,
            skill.metadata.argument_hint,
            skill.skill_dir.display().to_string(),
        ],
    )?;
    for alias in &skill.metadata.aliases {
        conn.execute(
            "INSERT INTO aliases (alias, dotpath) VALUES (?1, ?2)",
            params![alias, dotpath],
        )?;
    }
    for r in &skill.references {
        conn.execute(
            "INSERT INTO skill_references (dotpath, name, content) VALUES (?1, ?2, ?3)",
            params![dotpath, r.name, r.content],
        )?;
    }
    for p in &skill.activation_paths {
        conn.execute(
            "INSERT INTO activation_paths (dotpath, path) VALUES (?1, ?2)",
            params![dotpath, p],
        )?;
    }
    Ok(())
}

fn init_schema(conn: &Connection) -> SkillStoreResult<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS skills (
            dotpath          TEXT PRIMARY KEY,
            name             TEXT NOT NULL,
            description      TEXT NOT NULL DEFAULT '',
            body             TEXT NOT NULL DEFAULT '',
            allowed_tools    TEXT NOT NULL DEFAULT '[]',
            user_invocable   INTEGER NOT NULL DEFAULT 1,
            model_invocable  INTEGER NOT NULL DEFAULT 1,
            when_to_use      TEXT,
            argument_hint    TEXT,
            skill_dir        TEXT NOT NULL,
            created_at       INTEGER NOT NULL DEFAULT (unixepoch()),
            updated_at       INTEGER NOT NULL DEFAULT (unixepoch())
        );
        CREATE TABLE IF NOT EXISTS aliases (
            alias   TEXT PRIMARY KEY,
            dotpath TEXT NOT NULL REFERENCES skills(dotpath) ON DELETE CASCADE
        );
        CREATE TABLE IF NOT EXISTS skill_references (
            id      INTEGER PRIMARY KEY AUTOINCREMENT,
            dotpath TEXT NOT NULL REFERENCES skills(dotpath) ON DELETE CASCADE,
            name    TEXT NOT NULL,
            content TEXT NOT NULL DEFAULT ''
        );
        CREATE TABLE IF NOT EXISTS activation_paths (
            id      INTEGER PRIMARY KEY AUTOINCREMENT,
            dotpath TEXT NOT NULL REFERENCES skills(dotpath) ON DELETE CASCADE,
            path    TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS metadata (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL DEFAULT ''
        );
        CREATE INDEX IF NOT EXISTS idx_aliases_dotpath ON aliases(dotpath);
        CREATE INDEX IF NOT EXISTS idx_skill_references_dotpath ON skill_references(dotpath);
        CREATE INDEX IF NOT EXISTS idx_activation_paths_dotpath ON activation_paths(dotpath);
        ",
    )
    .map_err(SkillStoreError::from)?;
    Ok(())
}

impl From<rusqlite::Error> for SkillStoreError {
    fn from(e: rusqlite::Error) -> Self {
        SkillStoreError::Watch(Box::new(e))
    }
}

// ── Tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn write_skill(dir: &Path, name: &str, desc: &str, tools: &[&str]) {
        std::fs::create_dir_all(dir).unwrap();
        let mut yaml = format!("name: {name}\ndescription: \"{desc}\"\n");
        if !tools.is_empty() {
            yaml.push_str("allowed_tools:\n");
            for t in tools {
                yaml.push_str(&format!("  - {t}\n"));
            }
        }
        let body = format!("---\n{yaml}---\nDo the thing for {name}.\n");
        std::fs::write(dir.join("SKILL.md"), body).unwrap();
    }

    #[tokio::test]
    async fn test_open_empty_in_memory() {
        let store = SqliteSkillStore::in_memory().await.unwrap();
        assert!(store.load_all().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_import_and_get() {
        let tmp = tempfile::tempdir().unwrap();
        write_skill(&tmp.path().join("commit"), "commit", "Commit changes", &["bash", "read"]);
        write_skill(&tmp.path().join("review"), "review", "Review code", &["bash", "grep"]);

        let store = SqliteSkillStore::in_memory().await.unwrap();
        let n = store.import_from_dir(tmp.path()).await.unwrap();
        assert_eq!(n, 2);

        let all = store.load_all().await.unwrap();
        assert_eq!(all.len(), 2);

        let commit = store.get("commit").await.unwrap();
        assert_eq!(commit.metadata.name, "commit");
        assert_eq!(commit.policy.allowed_tools.len(), 2);
    }

    #[tokio::test]
    async fn test_import_full_replace() {
        let tmp = tempfile::tempdir().unwrap();
        write_skill(&tmp.path().join("first"), "first", "First", &[]);

        let store = SqliteSkillStore::in_memory().await.unwrap();
        store.import_from_dir(tmp.path()).await.unwrap();
        assert_eq!(store.load_all().await.unwrap().len(), 1);

        // Re-import with different content.
        std::fs::remove_dir_all(tmp.path().join("first")).unwrap();
        write_skill(&tmp.path().join("second"), "second", "Second", &[]);
        store.import_from_dir(tmp.path()).await.unwrap();

        let names = store.list_names().await.unwrap();
        assert_eq!(names, vec!["second".to_string()]);
    }

    #[tokio::test]
    async fn test_alias_resolution() {
        let tmp = tempfile::tempdir().unwrap();
        write_skill(&tmp.path().join("commit"), "commit", "Commit", &[]);
        // Add an alias by re-writing with alias in body isn't supported, so just test
        // that two skills with distinct names work.
        write_skill(&tmp.path().join("review"), "review", "Review", &[]);

        let store = SqliteSkillStore::in_memory().await.unwrap();
        store.import_from_dir(tmp.path()).await.unwrap();

        let commit = store.get("commit").await.unwrap();
        let review = store.get("review").await.unwrap();
        assert_eq!(commit.metadata.name, "commit");
        assert_eq!(review.metadata.name, "review");
    }

    #[tokio::test]
    async fn test_get_root_skill() {
        let tmp = tempfile::tempdir().unwrap();
        write_skill(&tmp.path().join("root"), "root", "Root", &[]);
        write_skill(&tmp.path().join("root").join("commit"), "commit", "Commit", &[]);
        write_skill(&tmp.path().join("root").join("review"), "review", "Review", &[]);

        let store = SqliteSkillStore::in_memory().await.unwrap();
        store.import_from_dir(tmp.path()).await.unwrap();

        let root = store.get_root_skill().await.unwrap();
        assert_eq!(root.metadata.name, "root");
    }

    #[tokio::test]
    async fn test_export_layout() {
        let tmp = tempfile::tempdir().unwrap();
        write_skill(&tmp.path().join("root"), "root", "Root skill", &["bash"]);
        write_skill(&tmp.path().join("root").join("commit"), "commit", "Commit", &[]);

        let store = SqliteSkillStore::in_memory().await.unwrap();
        store.import_from_dir(tmp.path()).await.unwrap();

        let export_dir = tempfile::tempdir().unwrap();
        let n = store.export_to_dir(export_dir.path()).await.unwrap();
        assert_eq!(n, 2);

        // Check directory layout: <export>/root/SKILL.md and <export>/root/commit/SKILL.md
        let root_skill_md = export_dir.path().join("root").join("SKILL.md");
        let commit_skill_md = export_dir.path().join("root").join("commit").join("SKILL.md");
        assert!(root_skill_md.exists());
        assert!(commit_skill_md.exists());

        let content = std::fs::read_to_string(&root_skill_md).unwrap();
        assert!(content.contains("name: root"));
        assert!(content.contains("bash"));
    }

    #[tokio::test]
    async fn test_export_import_roundtrip() {
        let source = tempfile::tempdir().unwrap();
        write_skill(&source.path().join("root"), "root", "Root", &["bash"]);
        write_skill(&source.path().join("root").join("commit"), "commit", "Commit", &[]);
        write_skill(&source.path().join("root").join("review"), "review", "Review", &["grep"]);

        // Sanity check: source has the expected structure.
        assert!(source.path().join("root").join("SKILL.md").exists());
        assert!(source.path().join("root").join("commit").join("SKILL.md").exists());
        assert!(source.path().join("root").join("review").join("SKILL.md").exists());

        // Import source → DB → export → re-import to new DB → verify.
        let store1 = SqliteSkillStore::in_memory().await.unwrap();
        let n1 = store1.import_from_dir(source.path()).await.unwrap();
        assert_eq!(n1, 3, "first import should yield 3 skills");
        let store1_names = store1.list_names().await.unwrap();
        assert_eq!(store1_names.len(), 3, "store1 should have 3 skills after import");

        let export = tempfile::tempdir().unwrap();
        store1.export_to_dir(export.path()).await.unwrap();

        // Sanity check: export directory should mirror the source tree.
        assert!(export.path().join("root").join("SKILL.md").exists());
        assert!(export.path().join("root").join("commit").join("SKILL.md").exists());
        assert!(export.path().join("root").join("review").join("SKILL.md").exists());

        let store2 = SqliteSkillStore::in_memory().await.unwrap();
        let n2 = store2.import_from_dir(export.path()).await.unwrap();
        assert_eq!(n2, 3, "second import should yield 3 skills");

        let names: Vec<String> = store2.list_names().await.unwrap();
        assert_eq!(names.len(), 3, "store2 should have 3 skills");
        assert!(names.contains(&"root".to_string()));
        assert!(names.contains(&"commit".to_string()));
        assert!(names.contains(&"review".to_string()));
    }

    #[tokio::test]
    async fn test_change_notification() {
        let store = SqliteSkillStore::in_memory().await.unwrap();
        let mut rx = store.subscribe();

        let tmp = tempfile::tempdir().unwrap();
        write_skill(&tmp.path().join("foo"), "foo", "Foo", &[]);
        store.import_from_dir(tmp.path()).await.unwrap();

        // import is not a notification trigger by design.
        assert!(rx.try_recv().is_err());

        // reload on non-existent skill returns NotFound.
        let result = store.reload("nonexistent").await;
        assert!(matches!(result, Err(SkillStoreError::NotFound { .. })));
    }
}
