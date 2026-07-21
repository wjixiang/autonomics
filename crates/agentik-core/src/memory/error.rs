use agentik_sdk::AnthropicError;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug, Error)]
pub enum Error {
    #[error("No item inside current memory")]
    EmptyMemoryItem,

    #[error("Failed to compact memory: {0}")]
    Compact(#[from] AnthropicError),

    #[error("no matching tool_use block found for tool_result (tool_use_id: {tool_use_id})")]
    OrphsnToolResult { tool_use_id: String },
}
