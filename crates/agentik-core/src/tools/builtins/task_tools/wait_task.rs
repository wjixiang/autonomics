use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

use agentik_proc::tool;
use agentik_sdk::types::ToolResult as AgentToolResult;
use async_trait::async_trait;

use crate::tools::task_runtime::{TaskEntry, TaskStatus};
use crate::tools::{ToolError, ToolFunction};

#[tool(
    name = "wait_task",
    description = "Block until the specified background task finishes, then return its result. \
                  If the task is already done, returns immediately. \
                  If the task does not exist, returns an error. \
                  If the timeout is reached before the task finishes, returns timeout status."
)]
pub struct WaitTaskInput {
    #[desc = "Tool call id of the background task to wait for"]
    task_id: String,
    #[desc = "Maximum seconds to wait for the task to finish. Defaults to 120."]
    #[default = 120]
    timeout_seconds: Option<u64>,
}

pub struct WaitTaskTool {
    tasks: Arc<RwLock<Vec<TaskEntry>>>,
}

impl WaitTaskTool {
    pub fn new(tasks: Arc<RwLock<Vec<TaskEntry>>>) -> Self {
        Self { tasks }
    }

    /// Read the actual result of a completed task.
    async fn read_result(
        &self,
        task_id: &str,
    ) -> Result<AgentToolResult, ToolError> {
        let tasks = self.tasks.read().await;
        let Some(task) = tasks.iter().find(|t| t.id() == task_id) else {
            return Ok(AgentToolResult::error(format!(
                "task {task_id:?} no longer exists"
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
            None => {
                // The status changed but no tool_result was stored.
                // This can happen if the task failed before storing a result.
                match task.status() {
                    TaskStatus::Failed(e) => Ok(AgentToolResult::success_json(serde_json::json!({
                        "task_id": task.id(),
                        "name": task.name(),
                        "status": "error",
                        "content": e.to_string(),
                    }))),
                    TaskStatus::Done(_) => Ok(AgentToolResult::success_json(serde_json::json!({
                        "task_id": task.id(),
                        "name": task.name(),
                        "status": "done",
                        "content": "(task completed but result was not stored)",
                    }))),
                    TaskStatus::Running => Ok(AgentToolResult::success_json(serde_json::json!({
                        "task_id": task.id(),
                        "name": task.name(),
                        "status": "running",
                        "content": "task is still running",
                    }))),
                }
            }
        }
    }
}

#[async_trait]
impl ToolFunction for WaitTaskTool {
    type Input = WaitTaskInput;

    fn sync_seconds(&self) -> u64 {
        // The tool itself should start quickly; the actual wait is handled
        // internally via tokio::select! with the user-specified timeout.
        5
    }

    fn timeout_seconds(&self) -> u64 {
        // Hard timeout well above the default user-specified timeout (120s),
        // so the select! inside run handles graceful timeout reporting.
        300
    }

    async fn run(&self, input: Self::Input) -> Result<AgentToolResult, ToolError> {
        let timeout_secs = input.timeout_seconds.unwrap_or(120);

        // Phase 1: look up the task; if already done, return immediately.
        let mut status_rx = {
            let tasks = self.tasks.read().await;
            let Some(task) = tasks.iter().find(|t| t.id() == input.task_id) else {
                return Ok(AgentToolResult::error(format!(
                    "no background task with id {:?}",
                    input.task_id
                )));
            };

            match task.status() {
                TaskStatus::Done(_) | TaskStatus::Failed(_) => {
                    // Already finished — read lock still held, but read_result
                    // will acquire its own read lock so we need to drop first.
                    drop(tasks);
                    return self.read_result(&input.task_id).await;
                }
                TaskStatus::Running => {}
            }

            // Still running — clone the receiver so we can wait outside the lock.
            task.status_receiver_clone()
        };
        // ^ read lock dropped here — the wait below is lock-free.

        // Phase 2: wait for status change or timeout.
        let completed = tokio::select! {
            result = status_rx.changed() => {
                result.is_ok()
            }
            _ = tokio::time::sleep(Duration::from_secs(timeout_secs)) => {
                false
            }
        };

        if completed {
            self.read_result(&input.task_id).await
        } else {
            // Timeout — task is still running.
            let tasks = self.tasks.read().await;
            let name = tasks
                .iter()
                .find(|t| t.id() == input.task_id)
                .map(|t| t.name().to_string())
                .unwrap_or_else(|| input.task_id.clone());

            Ok(AgentToolResult::success_json(serde_json::json!({
                "task_id": input.task_id,
                "name": name,
                "status": "timeout",
                "content": format!(
                    "task did not finish within {timeout_secs} seconds; it is still running"
                ),
            })))
        }
    }
}
