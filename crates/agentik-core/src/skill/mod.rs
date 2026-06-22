//! Skill system — a declarative, ordered workflow layered on top of tool
//! primitives, with integrated todo tracking.
//!
//! A [`Skill`] is a sequence of [`SkillStep`]s. Each step declares which
//! tools are available during that phase and the todos that must be
//! completed before the workflow advances. When a skill is attached to an
//! [`Agent`](crate::Agent):
//!
//! - the agent's normal request/response loop is preserved, but the LLM
//!   only *sees* (and may execute) the current step's `allowed_tools`
//!   (plus the always-available lifecycle and `update_todo` tools);
//! - a progress section describing the current step, its goal and its
//!   todos is injected into the system prompt each turn;
//! - the agent calls `update_todo` to mark progress, and the workflow
//!   **auto-advances** to the next step once every todo in the current
//!   step is completed.
//!
//! Skills are plain `serde` data, so they can be built in code or loaded
//! from config (TOML/JSON).
//!
//! ```
//! use agentik_core::skill::{Skill, SkillStep};
//!
//! let skill = Skill::new("research", "Read sources and summarize")
//!     .with_step(SkillStep {
//!         name: "gather".into(),
//!         goal: "Fetch the relevant papers".into(),
//!         allowed_tools: vec!["webfetch".into()],
//!         todos: vec!["find 3 sources".into()],
//!     })
//!     .with_step(SkillStep {
//!         name: "summarize".into(),
//!         goal: "Write the summary".into(),
//!         allowed_tools: vec!["write".into()],
//!         todos: vec!["draft summary".into()],
//!     });
//! ```

pub mod definition;
pub mod runtime;
pub mod todo_tool;

pub use definition::{Skill, SkillBuilder, SkillStep, step};
pub use runtime::{SkillRuntime, StepTransition, TodoItem, TodoStatus, UPDATE_TODO_TOOL};
pub use todo_tool::{TodoUpdateInput, TodoUpdateTool};

use std::sync::Arc;
use tokio::sync::Mutex;

/// Shared, thread-safe handle to an active skill's runtime state.
///
/// Held by both the [`Agent`](crate::Agent) (which reads the current step,
/// allowed tools and prompt section) and the [`TodoUpdateTool`] (which
/// mutates todo state).
pub type SharedSkillRuntime = Arc<Mutex<SkillRuntime>>;

/// Construct a [`SharedSkillRuntime`] plus its matching todo tool
/// registration. Intended for use by [`AgentBuilder`](crate::agent_builder::AgentBuilder).
pub(crate) fn instantiate(skill: Skill) -> (SharedSkillRuntime, crate::tools::ToolRegistration) {
    let runtime: SharedSkillRuntime = Arc::new(Mutex::new(SkillRuntime::new(skill)));
    let tool = TodoUpdateTool::new(Arc::clone(&runtime));
    (runtime, crate::tools::ToolRegistration::from(tool))
}
