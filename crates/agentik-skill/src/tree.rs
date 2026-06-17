//! Skill tree: hierarchical skill structure derived from directory layout.
//!
//! The filesystem hierarchy IS the tree — no extra configuration needed.
//! `skills/root/` is the root node; `skills/root/commit/` is a child.
//! Dotpath names (e.g. `root.commit`) are derived from relative paths.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::types::Skill;

/// A node in the skill tree.
#[derive(Debug, Clone)]
pub struct SkillTreeNode {
    pub skill: Skill,
    /// Dotpath identifier, e.g. "root", "root.commit", "root.review.deep".
    pub dotpath: String,
    /// Child skill nodes.
    pub children: Vec<SkillTreeNode>,
}

impl SkillTreeNode {
    /// Return the dotpath of the parent node, or None for root.
    pub fn parent_dotpath(&self) -> Option<&str> {
        if self.dotpath.contains('.') {
            let idx = self.dotpath.rfind('.').unwrap();
            Some(&self.dotpath[..idx])
        } else {
            None
        }
    }
}

/// The full skill tree built from loaded skills.
#[derive(Debug, Clone, Default)]
pub struct SkillTree {
    pub root: Option<SkillTreeNode>,
}

impl SkillTree {
    /// Build a skill tree from a flat list of skills and their skill_dirs.
    ///
    /// The `base_dir` is the common ancestor directory (e.g. the `skills/` dir).
    /// Each skill's `skill_dir` is resolved relative to `base_dir` to derive
    /// the dotpath and parent relationship.
    pub fn build(skills: Vec<Skill>, base_dir: &PathBuf) -> Self {
        // Compute dotpath for each skill based on relative path from base_dir.
        let mut dotpath_skills: Vec<(String, Skill)> = skills
            .into_iter()
            .filter_map(|skill| {
                let relative = skill.skill_dir.strip_prefix(base_dir).ok()?;
                let dotpath = relative
                    .components()
                    .filter_map(|c| c.as_os_str().to_str())
                    .collect::<Vec<_>>()
                    .join(".");
                if dotpath.is_empty() {
                    return None;
                }
                Some((dotpath, skill))
            })
            .collect();

        // Sort by dotpath depth (shorter first) for building order.
        dotpath_skills.sort_by_key(|(dp, _)| dp.matches('.').count());

        // Group by parent dotpath.
        let mut by_parent: HashMap<String, Vec<(String, Skill)>> = HashMap::new();
        let mut root_skill = None;

        for (dotpath, skill) in dotpath_skills {
            if !dotpath.contains('.') {
                // This is a root-level skill.
                if root_skill.is_none() {
                    root_skill = Some((dotpath, skill));
                } else {
                    tracing::warn!(
                        "multiple root skills found; using first one, ignoring '{dotpath}'"
                    );
                }
            } else {
                let parent = dotpath.rfind('.').map(|i| &dotpath[..i]).unwrap();
                by_parent
                    .entry(parent.to_string())
                    .or_default()
                    .push((dotpath, skill));
            }
        }

        // Recursively build the tree.
        let root = root_skill.map(|(dotpath, skill)| {
            let mut node = SkillTreeNode {
                skill,
                dotpath,
                children: Vec::new(),
            };
            build_children(&mut node, &by_parent);
            node
        });

        Self { root }
    }

    /// Look up a node by dotpath (e.g. "root", "root.commit").
    pub fn get(&self, dotpath: &str) -> Option<&SkillTreeNode> {
        let root = self.root.as_ref()?;

        let parts: Vec<&str> = dotpath.split('.').collect();
        if parts.is_empty() || parts[0] != root.dotpath {
            return None;
        }

        let mut current = root;
        for part in &parts[1..] {
            current = current
                .children
                .iter()
                .find(|c| c.dotpath.ends_with(part))?;
        }
        Some(current)
    }

    /// Generate an auto-generated children listing for the root skill's body.
    ///
    /// Lists each direct child's name, dotpath, description, and when_to_use.
    pub fn children_summary(&self) -> String {
        let root = match &self.root {
            Some(r) => r,
            None => return String::new(),
        };

        if root.children.is_empty() {
            return String::new();
        }

        let mut summary = String::from("\n\n## Available Sub-Skills\n\n");
        for child in &root.children {
            let when = child
                .skill
                .metadata
                .when_to_use
                .as_deref()
                .unwrap_or("Use when relevant.");
            summary.push_str(&format!(
                "### {} (`{}`)\n{}\nWhen to use: {}\n\n",
                child.skill.metadata.name, child.dotpath, child.skill.metadata.description, when
            ));
        }
        summary
    }

    /// Collect the union of all `allowed_tools` across every skill in the tree.
    ///
    /// This determines the full set of tools available to an Agent bound to this
    /// skill tree. The result is used to build a `Toolset` via `ToolProviderRegistry`.
    pub fn collect_all_allowed_tools(&self) -> std::collections::BTreeSet<String> {
        let mut tools = std::collections::BTreeSet::new();
        collect_allowed_from_node(&self.root, &mut tools);
        tools
    }
}

/// Recursively collect all `allowed_tools` from the tree into a single set.
fn collect_allowed_from_node(node: &Option<SkillTreeNode>, tools: &mut std::collections::BTreeSet<String>) {
    if let Some(node) = node {
        for t in &node.skill.policy.allowed_tools {
            tools.insert(t.clone());
        }
        for child in &node.children {
            collect_allowed_from_node(&Some(child.clone()), tools);
        }
    }
}

/// Recursively attach children to a node from the `by_parent` map.
fn build_children(node: &mut SkillTreeNode, by_parent: &HashMap<String, Vec<(String, Skill)>>) {
    if let Some(children) = by_parent.get(&node.dotpath) {
        for (dotpath, skill) in children {
            let mut child_node = SkillTreeNode {
                skill: skill.clone(),
                dotpath: dotpath.clone(),
                children: Vec::new(),
            };
            build_children(&mut child_node, by_parent);
            node.children.push(child_node);
        }
        // Sort children by dotpath for deterministic ordering.
        node.children.sort_by(|a, b| a.dotpath.cmp(&b.dotpath));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{SkillMetadata, SkillPolicy};
    use std::path::PathBuf;

    fn make_skill(name: &str, dir: &str) -> Skill {
        Skill {
            metadata: SkillMetadata {
                name: name.to_string(),
                description: format!("{} skill", name),
                aliases: Vec::new(),
                when_to_use: None,
                argument_hint: None,
                user_invocable: true,
                model_invocable: true,
            },
            policy: SkillPolicy::default(),
            body: String::new(),
            references: Vec::new(),
            activation_paths: Vec::new(),
            skill_dir: PathBuf::from(dir),
        }
    }

    #[test]
    fn test_build_simple_tree() {
        let base = PathBuf::from("/skills");
        let skills = vec![
            make_skill("root", "/skills/root"),
            make_skill("commit", "/skills/root/commit"),
            make_skill("review", "/skills/root/review"),
        ];

        let tree = SkillTree::build(skills, &base);
        let root = tree.root.unwrap();
        assert_eq!(root.dotpath, "root");
        assert_eq!(root.children.len(), 2);
        assert_eq!(root.children[0].dotpath, "root.commit");
        assert_eq!(root.children[1].dotpath, "root.review");
    }

    #[test]
    fn test_build_nested_tree() {
        let base = PathBuf::from("/skills");
        let skills = vec![
            make_skill("root", "/skills/root"),
            make_skill("commit", "/skills/root/commit"),
            make_skill("deep", "/skills/root/commit/deep"),
        ];

        let tree = SkillTree::build(skills, &base);
        let root = tree.root.unwrap();
        assert_eq!(root.children.len(), 1);
        assert_eq!(root.children[0].dotpath, "root.commit");
        assert_eq!(root.children[0].children.len(), 1);
        assert_eq!(root.children[0].children[0].dotpath, "root.commit.deep");
    }

    #[test]
    fn test_get_by_dotpath() {
        let base = PathBuf::from("/skills");
        let skills = vec![
            make_skill("root", "/skills/root"),
            make_skill("commit", "/skills/root/commit"),
        ];

        let tree = SkillTree::build(skills, &base);
        assert!(tree.get("root").is_some());
        assert!(tree.get("root.commit").is_some());
        assert!(tree.get("nonexistent").is_none());
        assert!(tree.get("root.nonexistent").is_none());
    }

    #[test]
    fn test_parent_dotpath() {
        let node = SkillTreeNode {
            skill: make_skill("commit", "/s/r/c"),
            dotpath: "root.commit".to_string(),
            children: Vec::new(),
        };
        assert_eq!(node.parent_dotpath(), Some("root"));

        let root_node = SkillTreeNode {
            skill: make_skill("root", "/s/r"),
            dotpath: "root".to_string(),
            children: Vec::new(),
        };
        assert_eq!(root_node.parent_dotpath(), None);
    }

    #[test]
    fn test_children_summary() {
        let base = PathBuf::from("/skills");
        let mut root_skill = make_skill("root", "/skills/root");
        root_skill.metadata.when_to_use = None;

        let mut commit_skill = make_skill("commit", "/skills/root/commit");
        commit_skill.metadata.when_to_use = Some("When the user asks to commit.".to_string());

        let skills = vec![root_skill, commit_skill];
        let tree = SkillTree::build(skills, &base);

        let summary = tree.children_summary();
        assert!(summary.contains("## Available Sub-Skills"));
        assert!(summary.contains("root.commit"));
        assert!(summary.contains("When the user asks to commit."));
    }

    #[test]
    fn test_collect_all_allowed_tools() {
        let base = PathBuf::from("/skills");
        let mut root_skill = make_skill("root", "/skills/root");
        root_skill.policy.allowed_tools.insert("activate_skill".to_string());
        root_skill.policy.allowed_tools.insert("attempt_complete".to_string());

        let mut commit_skill = make_skill("commit", "/skills/root/commit");
        commit_skill.policy.allowed_tools.insert("bash".to_string());
        commit_skill.policy.allowed_tools.insert("read".to_string());

        let mut review_skill = make_skill("review", "/skills/root/review");
        review_skill.policy.allowed_tools.insert("bash".to_string());
        review_skill.policy.allowed_tools.insert("grep".to_string());

        let tree = SkillTree::build(vec![root_skill, commit_skill, review_skill], &base);
        let tools = tree.collect_all_allowed_tools();

        // Union: activate_skill, attempt_complete, bash, read, grep
        assert_eq!(tools.len(), 5);
        assert!(tools.contains("activate_skill"));
        assert!(tools.contains("bash"));
        assert!(tools.contains("grep"));
        assert!(!tools.contains("webfetch")); // not declared
    }
}
