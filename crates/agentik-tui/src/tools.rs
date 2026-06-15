//! Tool-name → user-facing-string rendering.
//!
//! The renderer is parameterized on `&dyn AgentPanelTools`, so any
//! host can plug in its own renderer (or fall back to
//! [`DefaultAgentPanelTools`]).

use serde_json::Value;

/// Maps a `(tool_name, tool_input)` pair to a short, human-readable
/// label suitable for the panel's row hint and the expanded event
/// log. Implementations should fall back gracefully when the input
/// shape is unknown.
pub trait AgentPanelTools {
    fn user_facing_name(&self, name: &str, input: &Value) -> String;
}

/// Default implementation with built-in tool-name mappings.
/// Hosts with a different tool set can either (a) construct this and
/// call [`DefaultAgentPanelTools::with_extra`] to add their own rules,
/// or (b) implement `AgentPanelTools` from scratch.
#[derive(Debug, Clone, Default)]
pub struct DefaultAgentPanelTools {
    /// Extra rules tried first (so callers can override defaults).
    #[allow(dead_code)]
    extra: Vec<(String, String)>,
}

impl DefaultAgentPanelTools {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reserve room for the host to register custom tool names.
    #[allow(dead_code)]
    pub fn with_extra(_extra: Vec<(String, String)>) -> Self {
        Self::default()
    }
}

impl AgentPanelTools for DefaultAgentPanelTools {
    fn user_facing_name(&self, name: &str, input: &Value) -> String {
        tool_user_facing_name(name, input)
    }
}

pub(crate) fn tool_user_facing_name(name: &str, input: &Value) -> String {
    let first_str = |k: &str| -> Option<String> {
        input
            .as_object()
            .and_then(|o| o.get(k))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    };
    let first_id = |k: &str| -> Option<String> {
        first_str(k).map(|s| {
            if s.len() > 8 {
                format!("{}…", &s[..8])
            } else {
                s
            }
        })
    };
    let first_kv = || -> Option<String> {
        let obj = input.as_object()?;
        let (k, v) = obj.iter().next()?;
        Some(format!("{}: {}", k, format_value_short(v)))
    };

    match name {
        "kms_local" => first_str("path").map(|p| format!("Inspect {}", p)),
        "kms_create_knowledge" => {
            first_str("title").map(|t| format!("Create knowledge \"{}\"", truncate_inline(&t, 30)))
        }
        "kms_update_knowledge" => {
            first_str("id").map(|id| format!("Update knowledge {}", first_id("id").unwrap_or(id)))
        }
        "kms_rename_knowledge" => {
            first_str("title").map(|t| format!("Rename to \"{}\"", truncate_inline(&t, 30)))
        }
        "kms_delete_knowledge" => first_str("id")
            .map(|_| format!("Delete knowledge {}", first_id("id").unwrap_or_default())),
        "kms_get_knowledge" => first_id("id").map(|id| format!("Get knowledge {}", id)),
        "kms_search_entity" => {
            first_str("query").map(|q| format!("Search '{}'", truncate_inline(&q, 30)))
        }
        "kms_search_subtree" => {
            first_str("query").map(|q| format!("Search subtree '{}'", truncate_inline(&q, 30)))
        }
        "kms_get_entity" => first_id("id").map(|id| format!("Get entity {}", id)),
        "kms_get_entity_knowledge" => {
            first_id("entity_id").map(|id| format!("Get entity knowledge {}", id))
        }
        "kms_list_entities" => first_str("entity_type").map(|t| format!("List {} entities", t)),
        "kms_create_entity" => {
            first_str("name").map(|n| format!("Create entity \"{}\"", truncate_inline(&n, 30)))
        }
        "kms_update_entity" => first_id("id").map(|id| format!("Update entity {}", id)),
        "kms_delete_entity" => first_id("id").map(|id| format!("Delete entity {}", id)),
        "kms_create_index" => {
            first_str("title").map(|t| format!("Create group \"{}\"", truncate_inline(&t, 30)))
        }
        "kms_move_index" => {
            let from = first_str("index_path").unwrap_or_default();
            let to = first_str("new_parent_path").unwrap_or_default();
            if !from.is_empty() || !to.is_empty() {
                Some(format!(
                    "Move {} → under {}",
                    truncate_inline(&from, 30),
                    truncate_inline(&to, 30)
                ))
            } else {
                Some("Move index".to_string())
            }
        }
        "kms_delete_index" => first_id("id").map(|id| format!("Delete group {}", id)),
        "kms_navigate" => {
            first_str("target").map(|t| format!("Navigate to {}", truncate_inline(&t, 30)))
        }
        "kms_add_nomenclature" => {
            first_str("term").map(|t| format!("Nomenclature +\"{}\"", truncate_inline(&t, 30)))
        }
        "kms_update_nomenclature" => first_id("id").map(|id| format!("Nomenclature update {}", id)),
        "kms_delete_nomenclature" => first_id("id").map(|id| format!("Nomenclature delete {}", id)),
        "kms_link_orphans" => Some("Link orphans".to_string()),
        "kms_move_children" => {
            let source = first_str("source_path").unwrap_or_default();
            let remount = first_str("remount_path").unwrap_or_default();
            let title = first_str("new_group_title").unwrap_or_default();
            if !source.is_empty() || !remount.is_empty() || !title.is_empty() {
                Some(format!(
                    "Reorganize {} → group \"{}\" under {}",
                    truncate_inline(&source, 24),
                    truncate_inline(&title, 24),
                    truncate_inline(&remount, 24)
                ))
            } else {
                Some("Reorganize children".to_string())
            }
        }
        "kms_merge_subtree" => {
            first_str("target").map(|t| format!("Merge subtree → {}", truncate_inline(&t, 30)))
        }
        "kms_parallel_dispatch" => first_str("staging_title")
            .map(|t| format!("Dispatch subtask \"{}\"", truncate_inline(&t, 30))),
        _ => first_kv().map(|kv| format!("{} {}", name, kv)),
    }
    .unwrap_or_else(|| name.to_string())
}

pub(crate) fn truncate_inline(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}

pub(crate) fn format_value_short(v: &Value) -> String {
    match v {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => truncate_inline(s, 30),
        Value::Array(arr) => format!("[{} items]", arr.len()),
        Value::Object(_) => "{…}".to_string(),
    }
}

pub(crate) fn format_duration(d: std::time::Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m{}s", secs / 60, secs % 60)
    } else {
        format!("{}h{}m", secs / 3600, (secs % 3600) / 60)
    }
}

pub(crate) fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let end = s
        .char_indices()
        .take_while(|(i, _)| *i < max)
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(max);
    format!("{}…", &s[..end])
}
