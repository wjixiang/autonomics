use async_trait::async_trait;
use tokio::sync::mpsc::UnboundedSender;
use agentik_proc::tool;

use crate::agent::InternalEvent;
use agentik_sdk::types::tools::{ToolResult, ToolResultContent};

use crate::tools::{ToolError, ToolFunction, ToolRegistration};

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

/// Tool that requests the current session be aborted.
///
/// Holds a clone of the agent's [`InternalEvent`] sender so it can signal
/// [`InternalEvent::Abort`] dynamically at run time — the agent's event
/// loop honors the signal and ends the session without emitting `Done`.
pub struct AbortTaskTool {
    event_tx: UnboundedSender<InternalEvent>,
}

impl AbortTaskTool {
    pub fn new(event_tx: UnboundedSender<InternalEvent>) -> Self {
        Self { event_tx }
    }
}

#[async_trait]
impl ToolFunction for AbortTaskTool {
    type Input = AbortTaskInput;

    async fn run(&self, input: AbortTaskInput) -> Result<ToolResult, ToolError> {
        let _ = self.event_tx.send(InternalEvent::Abort);
        Ok(ToolResult {
            tool_use_id: "abort_task".to_string(),
            content: ToolResultContent::Text(format!("Task aborted. Reason: {}", input.reason)),
            is_error: None,
        })
    }
}

pub fn lifecycle_registrations(
    event_tx: UnboundedSender<InternalEvent>,
) -> Vec<ToolRegistration> {
    vec![ToolRegistration::from(AbortTaskTool::new(event_tx))]
}
