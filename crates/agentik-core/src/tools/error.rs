/// Tool execution errors.
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    /// Tool not found in registry.
    #[error("Tool '{name}' not found")]
    NotFound { name: String },

    /// Tool validation failed.
    #[error("Tool validation failed: {message}")]
    ValidationFailed { message: String },

    /// Tool execution failed.
    #[error("Tool execution failed: {source}")]
    ExecutionFailed {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Tool execution timed out.
    #[error("Tool execution timed out after {seconds} seconds")]
    Timeout { seconds: u64 },

    /// Tool registry error.
    #[error("Tool registry error: {message}")]
    RegistryError { message: String },
}

/// Result type for tool operations.
pub type ToolOperationResult<T> = Result<T, ToolError>;
