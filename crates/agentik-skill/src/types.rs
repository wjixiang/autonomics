use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

// ─── Skill：统一结构 ───
//
// 每一个 skill 对应文件系统上的一个目录：
//   skills/
//     commit/
//       SKILL.md          ← frontmatter + body
//       reference.md      ← 可选的参考文件
//
// 所有 skill，无论来源（bundled / 用户手写 / plugin），最终都物化为
// 同一个 Skill 结构体。区别仅在于 skill_dir 指向哪里。

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    /// 从 SKILL.md frontmatter 解析出的元数据
    pub metadata: SkillMetadata,
    /// 执行策略：工具限制
    pub policy: SkillPolicy,
    /// SKILL.md frontmatter 之后的正文（prompt 模板）
    pub body: String,
    /// 同目录下的参考文件，执行时注入 context
    pub references: Vec<ReferenceFile>,
    /// 条件激活 glob：只在操作匹配路径时激活此 skill
    pub activation_paths: Vec<String>,
    /// 磁盘路径，用于重载判断
    pub skill_dir: PathBuf,
}

impl Skill {
    /// 返回工具白名单切片，供 Toolset::execute 使用。空集合返回 None（不限制）。
    pub fn allowed_tools(&self) -> Option<Vec<String>> {
        if self.policy.allowed_tools.is_empty() {
            None
        } else {
            Some(self.policy.allowed_tools.iter().cloned().collect())
        }
    }
}

/// 同目录下的参考文件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferenceFile {
    pub name: String,
    pub content: String,
}

// ─── SkillMetadata ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMetadata {
    /// 唯一标识，如 "commit", "review-pr", "simplify"
    pub name: String,
    /// 简短描述
    pub description: String,
    /// 可选别名
    #[serde(default)]
    pub aliases: Vec<String>,
    /// 详细使用场景，帮助 LLM 决定是否调用
    pub when_to_use: Option<String>,
    /// 参数提示，如 "<file-path> [--fix]"
    pub argument_hint: Option<String>,
    /// 是否允许用户通过 /skill-name 调用
    #[serde(default = "default_true")]
    pub user_invocable: bool,
    /// 是否允许模型通过 SkillTool 调用
    #[serde(default = "default_true")]
    pub model_invocable: bool,
}

fn default_true() -> bool {
    true
}

// ─── SkillPolicy：执行策略 ───
//
// Skill 不直接调用 tool，而是声明执行时的约束。
// Runtime 据此过滤工具池。

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillPolicy {
    /// 允许的工具白名单（空 = 不限制）
    #[serde(default)]
    pub allowed_tools: BTreeSet<String>,
}

// ─── SkillFrontmatter ───
//
// SKILL.md 的 YAML frontmatter，与 SkillMetadata/SkillPolicy 一一对应。
// serde 反序列化后拆分为 Skill 的各个字段。

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillFrontmatter {
    // --- SkillMetadata 字段 ---
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub when_to_use: Option<String>,
    pub argument_hint: Option<String>,
    #[serde(default = "default_true")]
    pub user_invocable: bool,
    #[serde(default = "default_true")]
    pub model_invocable: bool,

    // --- SkillPolicy 字段 ---
    #[serde(default)]
    pub allowed_tools: Vec<String>,

    // --- 条件激活 ---
    #[serde(default)]
    pub paths: Vec<String>,
}

impl From<SkillFrontmatter> for Skill {
    fn from(fm: SkillFrontmatter) -> Self {
        Self {
            metadata: SkillMetadata {
                name: fm.name,
                description: fm.description,
                aliases: fm.aliases,
                when_to_use: fm.when_to_use,
                argument_hint: fm.argument_hint,
                user_invocable: fm.user_invocable,
                model_invocable: fm.model_invocable,
            },
            policy: SkillPolicy {
                allowed_tools: fm.allowed_tools.into_iter().collect(),
            },
            body: String::new(),    // 由 loader 填充
            references: Vec::new(), // 由 loader 填充
            activation_paths: fm.paths,
            skill_dir: PathBuf::new(), // 由 loader 填充
        }
    }
}

// ─── Error ───

#[derive(Debug, thiserror::Error)]
pub enum SkillError {
    #[error("skill '{name}' not found")]
    NotFound { name: String },

    #[error("skill '{name}' is not user-invocable")]
    NotUserInvocable { name: String },

    #[error("skill '{name}' is not model-invocable")]
    NotModelInvocable { name: String },

    #[error("failed to parse SKILL.md frontmatter in {path}: {reason}")]
    ParseFailed { path: PathBuf, reason: String },

    #[error("IO error reading skill at {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}
