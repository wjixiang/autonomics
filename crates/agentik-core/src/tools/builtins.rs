//! Built-in primitive tools: shell + filesystem operations.
//!
//! This module groups the concrete tool implementations, keeping them
//! separate from the tools *framework* (trait, registry, executor) which
//! lives in the parent [`crate::tools`] module.

pub mod bash;
pub mod edit;
pub mod glob;
pub mod grep;
pub mod lifecycle;
pub mod read;
pub mod write;

pub use bash::{BashInput, BashTool};
pub use edit::{EditInput, EditTool};
pub use glob::{GlobInput, GlobTool};
pub use grep::{GrepInput, GrepTool};
pub use lifecycle::{
    AbortTaskInput, AbortTaskTool, AttemptCompleteInput, AttemptCompleteTool,
    lifecycle_registrations,
};
pub use read::{ReadInput, ReadTool};
pub use write::{WriteInput, WriteTool};

use super::{ToolRegistration, Toolset};

/// The foundational file/shell primitive tools: bash, read, write, edit,
/// glob, grep. Wire these into an agent's toolset for basic filesystem
/// and command capability.
pub fn primitive_registrations() -> Vec<ToolRegistration> {
    vec![
        ToolRegistration::from(BashTool),
        ToolRegistration::from(ReadTool),
        ToolRegistration::from(WriteTool),
        ToolRegistration::from(EditTool),
        ToolRegistration::from(GlobTool),
        ToolRegistration::from(GrepTool),
    ]
}

/// Convenience: a fresh `Toolset` pre-loaded with the primitive tools
/// and the lifecycle (attempt_complete / abort_task) tools.
pub fn default_toolset() -> Toolset {
    let mut toolset = Toolset::new();
    let _ = toolset.register_all(primitive_registrations());
    let _ = toolset.register_all(lifecycle_registrations());
    toolset
}
