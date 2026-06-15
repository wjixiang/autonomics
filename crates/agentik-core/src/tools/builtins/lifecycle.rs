use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use agentik_sdk::types::ToolEffect;
use agentik_sdk::types::tools::{ToolResult, ToolResultContent};

use crate::tools::{ToolError, ToolFunction, ToolRegistration};

#[derive(Debug, Deserialize, Serialize, agentik_proc::ToolInput)]
#[tool(name = "attempt_complete", description = "Signal that the ENTIRE user request is fulfilled. Only call this when every part of the request has been completed. Do NOT call this for intermediate steps.")]
pub struct AttemptCompleteInput {
    #[desc = "Brief explanation of why the task is complete"]
    pub reason: String,
}

pub struct AttemptCompleteTool;

#[async_trait]
impl ToolFunction for AttemptCompleteTool {
    type Input = AttemptCompleteInput;

    fn effects(&self) -> Vec<ToolEffect> {
        vec![ToolEffect::AttemptComplete]
    }

    async fn run(&self, input: AttemptCompleteInput) -> Result<ToolResult, ToolError> {
        Ok(ToolResult {
            tool_use_id: "attempt_complete".to_string(),
            content: ToolResultContent::Text(format!(
                "Task completed successfully. Reason: {}",
                input.reason
            )),
            is_error: None,
        })
    }
}

#[derive(Debug, Deserialize, Serialize, agentik_proc::ToolInput)]
#[tool(
    name = "abort_task",
    description = "Signal that the current task cannot or should not be completed. \
                  Call this when the task is impossible, blocked irrecoverably, \
                  or the user explicitly requests cancellation."
)]
pub struct AbortTaskInput {
    #[desc = "Explanation of why the task is being aborted"]
    pub reason: String,
}

pub struct AbortTaskTool;

#[async_trait]
impl ToolFunction for AbortTaskTool {
    type Input = AbortTaskInput;

    fn effects(&self) -> Vec<ToolEffect> {
        vec![ToolEffect::Abort]
    }

    async fn run(&self, input: AbortTaskInput) -> Result<ToolResult, ToolError> {
        Ok(ToolResult {
            tool_use_id: "abort_task".to_string(),
            content: ToolResultContent::Text(format!(
                "Task aborted. Reason: {}",
                input.reason
            )),
            is_error: None,
        })
    }
}

pub fn lifecycle_registrations() -> Vec<ToolRegistration> {
    vec![
        ToolRegistration::from(AttemptCompleteTool),
        ToolRegistration::from(AbortTaskTool),
    ]
}
