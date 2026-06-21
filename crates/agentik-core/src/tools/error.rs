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

impl From<anyhow::Error> for ToolError {
    fn from(value: anyhow::Error) -> Self {
        ToolError::ExecutionFailed {
            source: value.into(),
        }
    }
}

impl From<&str> for ToolError {
    fn from(message: &str) -> Self {
        ToolError::ValidationFailed {
            message: message.to_string(),
        }
    }
}

impl From<String> for ToolError {
    fn from(message: String) -> Self {
        ToolError::ValidationFailed { message }
    }
}

impl From<serde_json::Error> for ToolError {
    fn from(e: serde_json::Error) -> Self {
        ToolError::ExecutionFailed {
            source: Box::new(e),
        }
    }
}

/// Result type for tool operations.
pub type ToolOperationResult<T> = Result<T, ToolError>;
