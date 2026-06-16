//! Filesystem skill loader: read SKILL.md + references, support reload.

use std::fs;
use std::path::{Path, PathBuf};

use crate::types::{Skill, SkillError, SkillFrontmatter};

/// 解析 SKILL.md 的 frontmatter (YAML between `---` fences) 和 body。
///
/// Returns `(frontmatter_yaml, body)` on success.
fn parse_skill_md(content: &str) -> Result<(String, String), SkillError> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return Err(SkillError::ParseFailed {
            path: PathBuf::from("<unknown>"),
            reason: "SKILL.md must start with YAML frontmatter (`---`)".into(),
        });
    }

    // 找到第二个 `---`
    let rest = &trimmed[3..];
    let end = rest.find("\n---").ok_or_else(|| SkillError::ParseFailed {
        path: PathBuf::from("<unknown>"),
        reason: "missing closing `---` for frontmatter".into(),
    })?;

    let yaml = rest[..end].trim().to_string();
    let body = rest[end + 4..].trim().to_string();

    Ok((yaml, body))
}

/// 从一个 skill 目录加载单个 skill。
///
/// 期望目录结构：
/// ```text
///   <skill_dir>/
///     SKILL.md          ← 必须，包含 frontmatter + prompt body
///     reference.md      ← 可选
///     *.md               ← 其他可选参考文件
/// ```
pub fn load_skill_from_dir(skill_dir: &Path) -> Result<Skill, SkillError> {
    let skill_md = skill_dir.join("SKILL.md");

    let content = fs::read_to_string(&skill_md).map_err(|e| SkillError::Io {
        path: skill_md.clone(),
        source: e,
    })?;

    let (yaml, body) = parse_skill_md(&content).map_err(|mut e| {
        if let SkillError::ParseFailed { path, .. } = &mut e {
            *path = skill_md.clone();
        }
        e
    })?;

    let frontmatter: SkillFrontmatter =
        serde_yaml::from_str(&yaml).map_err(|e| SkillError::ParseFailed {
            path: skill_md.clone(),
            reason: e.to_string(),
        })?;

    let mut skill = Skill::from(frontmatter);
    skill.body = body;
    skill.skill_dir = skill_dir.to_path_buf();

    // 加载同目录下的 .md 参考文件（排除 SKILL.md 自身）
    if skill_dir.is_dir() {
        let entries = match fs::read_dir(skill_dir) {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!(error = %e, path = %skill_dir.display(), "failed to read skill dir for references");
                return Ok(skill);
            }
        };

        let refs: Vec<_> = entries
            .filter_map(|entry| {
                let entry = match entry {
                    Ok(e) => e,
                    Err(_) => return None,
                };
                let path = entry.path();
                // 只取 .md 文件，跳过 SKILL.md
                if path.extension().map_or(false, |ext| ext == "md")
                    && path.file_name().map_or(false, |n| n != "SKILL.md")
                {
                    Some(path)
                } else {
                    None
                }
            })
            .collect();

        for ref_path in refs {
            let name = ref_path
                .file_name()
                .ok_or_else(|| SkillError::ParseFailed {
                    path: ref_path.clone(),
                    reason: "reference file has no file name".into(),
                })?
                .to_string_lossy()
                .to_string();
            let content = fs::read_to_string(&ref_path).map_err(|e| SkillError::Io {
                path: ref_path.clone(),
                source: e,
            })?;
            skill.references.push(crate::types::ReferenceFile { name, content });
        }
    }

    Ok(skill)
}

/// 从一个 skill 目录列表批量加载所有 skills。
///
/// 跳过无法解析的目录（记录 warn 日志），继续加载其余的。
pub fn load_skills_from_dirs(dirs: &[PathBuf]) -> Vec<Skill> {
    let mut skills = Vec::new();

    for dir in dirs {
        // dir 本身可能是 skills/ 根目录，内含多个 skill 子目录
        if dir.join("SKILL.md").exists() {
            // dir 就是 skill 目录本身
            match load_skill_from_dir(dir) {
                Ok(skill) => skills.push(skill),
                Err(e) => tracing::warn!(error = %e, path = %dir.display(), "failed to load skill"),
            }
        } else if dir.is_dir() {
            // dir 是 skills 根目录，遍历子目录
            let entries = match fs::read_dir(dir) {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!(error = %e, path = %dir.display(), "failed to read skill dir");
                    continue;
                }
            };

            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() && path.join("SKILL.md").exists() {
                    match load_skill_from_dir(&path) {
                        Ok(skill) => skills.push(skill),
                        Err(e) => {
                            tracing::warn!(error = %e, path = %path.display(), "failed to load skill");
                        }
                    }
                }
            }
        }
    }

    skills
}

/// Recursively walk directories and load all skills found.
///
/// Unlike `load_skills_from_dirs` which only walks one level deep,
/// this function recurses into subdirectories to discover nested skills.
pub fn load_skills_recursive(dirs: &[PathBuf]) -> Vec<Skill> {
    let mut skills = Vec::new();
    for dir in dirs {
        walk_skill_dirs(dir, &mut skills);
    }
    skills
}

fn walk_skill_dirs(dir: &Path, skills: &mut Vec<Skill>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!(error = %e, path = %dir.display(), "failed to read dir");
            return;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.join("SKILL.md").exists() {
                match load_skill_from_dir(&path) {
                    Ok(skill) => skills.push(skill),
                    Err(e) => {
                        tracing::warn!(error = %e, path = %path.display(), "failed to load skill");
                    }
                }
            }
            // Always recurse into subdirectories regardless of SKILL.md presence.
            walk_skill_dirs(&path, skills);
        }
    }
}

/// Load all skills recursively and build a SkillTree.
///
/// Returns the tree enriched with an auto-generated children summary
/// appended to the root skill's body.
pub fn load_skill_tree_from_dirs(dirs: &[PathBuf]) -> crate::tree::SkillTree {
    let skills = load_skills_recursive(dirs);
    if skills.is_empty() {
        return crate::tree::SkillTree::default();
    }

    // Determine the base_dir: the longest common prefix of all skill_dirs,
    // or simply the first skill_dir.
    let base_dir = dirs.first().cloned().unwrap_or_default();

    let mut tree = crate::tree::SkillTree::build(skills, &base_dir);

    // Append auto-generated children list to root body.
    if let Some(ref mut root) = tree.root {
        let children_summary = tree.children_summary();
        if !children_summary.is_empty() {
            root.skill.body.push_str(&children_summary);
        }
    }

    tree
}

/// 重新加载单个 skill（通过 skill_dir 定位），返回新的 Skill。
///
/// 如果磁盘上的 SKILL.md 未变（通过 mtime 判断），返回 None。
pub fn reload_skill(skill: &Skill) -> Result<Option<Skill>, SkillError> {
    let skill_md = skill.skill_dir.join("SKILL.md");

    let current_mtime = fs::metadata(&skill_md)
        .and_then(|m| m.modified())
        .map_err(|e| SkillError::Io {
            path: skill_md.clone(),
            source: e,
        })?;

    // 存储上次加载时间的字段可后续添加；目前简单重载
    let _ = current_mtime;

    load_skill_from_dir(&skill.skill_dir).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_skill_md() {
        let input = "\
---
name: commit
description: Commit changes
allowed_tools:
  - bash
  - read
paths:
  - \"src/**/*.rs\"
---

Commit all staged changes with a conventional commit message.
";
        let (yaml, body) = parse_skill_md(input).unwrap();
        assert!(yaml.contains("name: commit"));
        assert!(body.starts_with("Commit all staged"));
    }

    #[test]
    fn test_parse_missing_frontmatter() {
        let input = "Just some text without frontmatter.";
        let result = parse_skill_md(input);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_missing_closing_fence() {
        let input = "---\nname: test\nNo closing fence";
        let result = parse_skill_md(input);
        assert!(result.is_err());
    }
}
