//! Shared wire types for the agentik control plane.
//!
//! These types are the **contract** between the runtime backend
//! (`agentik-runtime` daemon) and any frontend (CLI, TUI, future web). They
//! live in this lightweight crate — depending only on `agentik-types`, `uuid`,
//! and `serde` — so a frontend binary can consume them **without linking**
//! `agentik-core` or `agentik-runtime`.
//!
//! `agentik-core` and `agentik-runtime` re-export these definitions from their
//! historical paths so existing call sites keep compiling.

use uuid::Uuid;

// Re-export the wire types the control plane carries verbatim. These are
// defined in `agentik-types`; re-exporting here gives frontends a single crate
// to depend on.
pub use agentik_types::{AgentEvent, AgentUiEvent, ContentBlock, ContentBlockKind, Message};

pub mod discovery;
pub use discovery::{agentik_dir, daemon_json_path, read_daemon_info, state_dir, DaemonInfo};

// ── Agent lifecycle ──────────────────────────────────────────────

/// Coarse lifecycle state of an agent.
///
/// (Historically defined in `agentik_core::lifecycle`; moved here so it can
/// cross the control-plane boundary without linking core.)
#[derive(Debug, PartialEq, Eq, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(rename_all = "UPPERCASE")]
pub enum AgentLifecycleStatus {
    IDLE,
    RUNNING,
    ABORTED,
}

// ── Process events ──────────────────────────────────────────────

/// Event emitted by the [`AgentManager`](https://docs.rs) aggregated stream.
///
/// Wraps agent-level events with the source agent's identity and adds
/// lifecycle-state-change and process-exit events that only the manager can
/// produce.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
pub enum ProcessEvent {
    /// An agent-level event, tagged with the source agent's ID.
    Agent {
        agent_id: Uuid,
        /// The agent-level event. Represented as free-form JSON in the schema
        /// to avoid pulling the full `AgentEvent` → `Message` type tree into
        /// the doc; consumers should consult the `AgentEvent` enum in
        /// `agentik-types` for the exact variants.
        #[cfg_attr(feature = "schema", schema(value_type = serde_json::Value))]
        event: AgentUiEvent,
    },

    /// An agent's lifecycle state changed.
    StateChanged {
        agent_id: Uuid,
        new_status: AgentLifecycleStatus,
    },

    /// An agent process exited (completed, aborted, or crashed).
    ProcessExited {
        agent_id: Uuid,
        status: ProcessExitStatus,
    },
}

/// Describes how an agent process exited.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
pub enum ProcessExitStatus {
    /// `agent.start()` returned `Ok(())`.
    Completed,
    /// `agent.start()` returned `Err`.
    Error(String),
    /// The tokio task panicked.
    Panicked(String),
    /// Cancelled via `CancellationToken`.
    Cancelled,
    /// Explicitly stopped via a `Stop` command.
    Stopped,
}

// ── Spawn options ───────────────────────────────────────────────

/// Options the frontend supplies when spawning an agent by kind.
///
/// Contains only serialisable plain data — no `agentik-core` types.
#[derive(Default, Clone, Debug, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
pub struct AgentSpawnOpts {
    /// Override the system-prompt identity line.
    pub system_prompt_identity: Option<String>,

    /// Override the system-prompt section (task-specific instructions).
    pub system_prompt_section: Option<String>,

    /// Optional initial user message injected right after spawn.
    pub initial_message: Option<Vec<ContentBlock>>,
}

// ── Declarative model configuration ─────────────────────────────

/// A single provider entry, persisted by the frontend / host.
#[derive(serde::Serialize, serde::Deserialize, Default, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
pub struct ProviderConfig {
    /// Stable unique id; referenced by [`PoolEntry::provider_id`].
    pub id: String,
    /// User-chosen display name (e.g. "mimo-prod").
    pub display_name: String,
    /// Which built-in provider type this instantiates
    /// (`"mimo"`, `"minimax"`, `"sensenova"`, `"deepseek"`, `"zai"`).
    pub provider_type: String,
    /// API key for this provider.
    pub api_key: String,
    /// Base URL. Empty string means "use the SDK's built-in default".
    #[serde(default)]
    pub base_url: String,
    /// User-curated model list for this provider. Empty = let the SDK pick.
    #[serde(default)]
    pub models: Vec<String>,
}

/// A single model entry in the pool. References a [`ProviderConfig`] by its
/// stable [`ProviderConfig::id`].
#[derive(serde::Serialize, serde::Deserialize, Default, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
pub struct PoolEntry {
    pub provider_id: String,
    pub model: String,
}

/// Top-level model configuration passed by the frontend.
#[derive(serde::Serialize, serde::Deserialize, Default, Clone, Debug)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
pub struct ModelConfig {
    #[serde(default)]
    pub providers: Vec<ProviderConfig>,
    #[serde(default)]
    pub pool: Vec<PoolEntry>,
}

impl ModelConfig {
    /// Deserialise from a JSON string.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Serialise to a pretty JSON string.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

// ── Skill tree (control-plane wire types) ───────────────────────
//
// Plain serde mirrors of `agentik_skill` domain types. Defined here so the
// control plane can carry skills over the wire without forcing frontends to
// depend on `agentik-skill`. The runtime maps `agentik_skill::Skill` ↔ these.

/// A reference file attached to a skill (mirrors `agentik_skill::ReferenceFile`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
pub struct SkillReferenceWire {
    pub name: String,
    pub content: String,
}

/// A skill, serialised for the control plane (mirrors `agentik_skill::Skill`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
pub struct SkillWire {
    /// Dotted hierarchical path, e.g. `"root.commit"`.
    pub dotpath: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub when_to_use: Option<String>,
    #[serde(default)]
    pub argument_hint: Option<String>,
    pub user_invocable: bool,
    pub model_invocable: bool,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub references: Vec<SkillReferenceWire>,
    #[serde(default)]
    pub activation_paths: Vec<String>,
}

/// A node in the skill tree (mirrors `agentik_skill::SkillTreeNode`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
pub struct SkillTreeNodeWire {
    pub skill: SkillWire,
    pub dotpath: String,
    /// Child nodes. On the wire this is `Vec<SkillTreeNodeWire>` (arbitrary
    /// depth). In the OpenAPI schema it is rendered as free-form JSON, because
    /// `utoipa` cannot express the direct `Vec<Self>` recursion without
    /// infinite expansion; each array element is a `SkillTreeNodeWire`.
    #[cfg_attr(feature = "schema", schema(value_type = Vec<serde_json::Value>))]
    #[serde(default)]
    pub children: Vec<SkillTreeNodeWire>,
}

/// A skill change notification streamed from the store.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(rename_all = "UPPERCASE")]
pub enum SkillChangeWire {
    Added,
    Modified,
    Removed,
}

impl SkillChangeWire {
    pub fn skill_name(self, name: impl Into<String>) -> SkillChangeNotificationWire {
        SkillChangeNotificationWire {
            change_type: self,
            skill_name: name.into(),
        }
    }
}

/// A skill change event with the affected skill name.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
pub struct SkillChangeNotificationWire {
    pub change_type: SkillChangeWire,
    pub skill_name: String,
}
