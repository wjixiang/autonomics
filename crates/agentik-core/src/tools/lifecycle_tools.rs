use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use agentik_types::ToolEffect;
use agentik_types::tools::{ToolBuilder, ToolResult, ToolResultContent};

use super::toolset::ToolRegistration;
use super::ToolFunction;

#[derive(Debug, Deserialize)]
struct ReasonInput {
    reason: String,
}

pub struct AttemptCompleteTool;

#[async_trait]
impl ToolFunction for AttemptCompleteTool {
    fn definition(&self) -> agentik_types::Tool {
        ToolBuilder::new("attempt_complete", "Signal that the ENTIRE user request is fulfilled. Only call this when every part of the request has been completed. Do NOT call this for intermediate steps.")
            .parameter("reason", "string", "Brief explanation of why the task is complete")
            .required("reason")
            .build()
    }

    fn effects(&self) -> Vec<ToolEffect> {
        vec![ToolEffect::AttemptComplete]
    }

    async fn execute(
        &self,
        input: Value,
    ) -> Result<ToolResult, Box<dyn std::error::Error + Send + Sync>> {
        let input: ReasonInput = serde_json::from_value(input)?;
        Ok(ToolResult {
            tool_use_id: "attempt_complete".to_string(),
            content: ToolResultContent::Text(format!("Task completed successfully. Reason: {}", input.reason)),
            is_error: None,
        })
    }
}

pub struct AbortTaskTool;

#[async_trait]
impl ToolFunction for AbortTaskTool {
    fn definition(&self) -> agentik_types::Tool {
        ToolBuilder::new(
            "abort_task",
            "Signal that the current task cannot or should not be completed. \
             Call this when the task is impossible, blocked irrecoverably, \
             or the user explicitly requests cancellation.",
        )
        .parameter("reason", "string", "Explanation of why the task is being aborted")
        .required("reason")
        .build()
    }

    fn effects(&self) -> Vec<ToolEffect> {
        vec![ToolEffect::Abort]
    }

    async fn execute(
        &self,
        input: Value,
    ) -> Result<ToolResult, Box<dyn std::error::Error + Send + Sync>> {
        let input: ReasonInput = serde_json::from_value(input)?;
        Ok(ToolResult {
            tool_use_id: "abort_task".to_string(),
            content: ToolResultContent::Text(format!("Task aborted. Reason: {}", input.reason)),
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
