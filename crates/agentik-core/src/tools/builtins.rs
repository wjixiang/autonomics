//! Built-in lifecycle tools: attempt_complete, abort_task.
//!
//! These are the always-injected tools that the agent framework requires
//! for task signaling. Primitive tools (bash, read, write, etc.) live in
//! the `agentik-tools` crate.

pub mod lifecycle;
pub mod skill;

pub use lifecycle::{
    AbortTaskInput, AbortTaskTool, AttemptCompleteInput, AttemptCompleteTool,
    lifecycle_registrations,
};
pub use skill::{SkillActivationState, skill_registration};
