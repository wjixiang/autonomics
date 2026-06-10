pub mod lifecycle_tools;
pub mod error;
pub mod executor;
pub mod function;
pub mod registry;
pub mod toolset;

pub use error::{ToolError, ToolOperationResult};
pub use executor::{ToolExecutionConfig, ToolExecutionConfigBuilder, ToolExecutor};
pub use function::{SimpleTool, ToolFunction};
pub use registry::{SharedToolRegistry, ToolRegistry};
pub use toolset::{ToolRegistration, Toolset};

pub use agentik_types::{
    Tool, ToolBuilder, ToolChoice, ToolEffect, ToolUse, ToolResult, ToolResultContent,
    ToolValidationError,
};

pub use lifecycle_tools::{AbortTaskTool, AttemptCompleteTool, lifecycle_registrations};

#[macro_export]
macro_rules! tool_function {
    (|$input:ident: Value| $body:expr) => {
        $crate::tools::function::SimpleTool::new(move |$input: Value| Box::pin($body))
    };
}
