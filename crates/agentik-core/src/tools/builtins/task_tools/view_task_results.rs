use std::sync::Arc;
use tokio::sync::RwLock;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use agentik_sdk::types::ToolResult as AgentToolResult;

use crate::tools::task_runtime::TaskEntry;
use crate::tools::{ToolError, ToolFunction};

#[derive(Debug, Deserialize, Serialize, agentik_proc::ToolInput)]
#[tool(
    name = "view_task_results",
    description = "View the stored result of a single background task by its tool call id. \
                  Returns the real tool result recorded inside the task entry. \
                  If the task has not finished yet, reports it as still running."
)]
pub struct ViewTaskResultsInput {
    #[desc = "Tool call id of the target background task"]
    task_id: String,
}

pub struct TaskResultViewerTool {
    tasks: Arc<RwLock<Vec<TaskEntry>>>,
}

impl TaskResultViewerTool {
    pub fn new(tasks: Arc<RwLock<Vec<TaskEntry>>>) -> Self {
        Self { tasks }
    }
}

#[async_trait]
impl ToolFunction for TaskResultViewerTool {
    type Input = ViewTaskResultsInput;

    fn sync_seconds(&self) -> u64 {
        10
    }

    async fn run(&self, input: Self::Input) -> Result<AgentToolResult, ToolError> {
        let tasks = self.tasks.read().await;
        let Some(task) = tasks.iter().find(|t| t.id() == input.task_id) else {
            return Ok(AgentToolResult::error(format!(
                "no background task with id {:?}",
                input.task_id
            )));
        };

        match task.tool_result() {
            Some(result) => {
                // Result consumed — mark the task as read so the toolset can
                // reclaim its entry on the next execution pass.
                task.mark_read();
                let is_error = result.is_error.unwrap_or(false);
                Ok(AgentToolResult::success_json(serde_json::json!({
                    "task_id": task.id(),
                    "name": task.name(),
                    "status": if is_error { "error" } else { "done" },
                    "content": result.text_content(),
                })))
            }
            // Still running (or failed before storing a result) — leave the
            // entry unread so it can be queried again later.
            None => Ok(AgentToolResult::success_json(serde_json::json!({
                "task_id": task.id(),
                "name": task.name(),
                "status": "running",
                "content": "task has not produced a result yet",
            }))),
        }
    }
}
