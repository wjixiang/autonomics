//! Tool framework (trait, registry, executor) and built-in tools.
//!
//! - The *framework* modules (`function`, `registry`, `toolset`,
//!   `executor`, `error`) define how tools are declared and dispatched.
//! - [`builtins`] holds the concrete built-in tool implementations.

pub mod builtins;
pub mod error;
pub mod executor;
pub mod function;
pub mod registry;
pub mod toolset;

pub use error::{ToolError, ToolOperationResult};
pub use executor::{ToolExecutionConfig, ToolExecutionConfigBuilder, ToolExecutor};
pub use function::{DynToolFunction, ToolFunction};
pub use registry::{SharedToolRegistry, ToolRegistry};
pub use toolset::{ToolRegistration, Toolset};

pub use agentik_sdk::types::{
    Tool, ToolBuilder, ToolChoice, ToolEffect, ToolResult, ToolResultContent, ToolUse,
    ToolValidationError,
};

// Re-export the built-in tools and registration helpers at the `tools`
// facade so callers can do `use agentik_core::tools::{BashTool, ...}`.
pub use builtins::{
    AbortTaskInput, AbortTaskTool, AttemptCompleteInput, AttemptCompleteTool, BashInput, BashTool,
    EditInput, EditTool, GlobInput, GlobTool, GrepInput, GrepTool, ReadInput, ReadTool,
    WriteInput, WriteTool, default_toolset, lifecycle_registrations, primitive_registrations,
};
