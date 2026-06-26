use std::sync::Arc;
use tokio::sync::Mutex;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use agentik_sdk::types::ToolResult as AgentToolResult;

use crate::tools::{ToolError, ToolFunction, ToolRegistration};
use crate::tools::task_runtime::{TaskEntry, TaskStatus};

#[derive(Debug, Deserialize, Serialize, agentik_proc::ToolInput)]
#[tool(
    name = "view_task_results",
    description = "View results of finished background tasks. \
                  Returns content for any tasks that have completed since the last check."
)]
pub struct ViewTaskResultsInput {}

pub struct TaskResultViewerTool {
    tasks: Arc<Mutex<Vec<TaskEntry>>>,
}

impl TaskResultViewerTool {
    pub fn new(tasks: Arc<Mutex<Vec<TaskEntry>>>) -> Self {
        Self { tasks }
    }
}

#[async_trait]
impl ToolFunction for TaskResultViewerTool {
    type Input = ViewTaskResultsInput;

    fn sync_seconds(&self) -> u64 {
        1
    }

    async fn run(&self, _input: Self::Input) -> Result<AgentToolResult, ToolError> {
        let mut tasks = self.tasks.lock().await;
        let mut results = Vec::new();

        for entry in tasks.iter_mut() {
            if entry.is_read() {
                continue;
            }
            match entry.status() {
                TaskStatus::Done(ref result) => {
                    entry.mark_read();
                    results.push(serde_json::json!({
                        "task_id": entry.id(),
                        "status": "done",
                        "content": result.text_content(),
                    }));
                }
                TaskStatus::Failed(ref err) => {
                    entry.mark_read();
                    results.push(serde_json::json!({
                        "task_id": entry.id(),
                        "status": "error",
                        "error": err.to_string(),
                    }));
                }
                _ => {}
            }
        }

        Ok(AgentToolResult::success_json(serde_json::json!({
            "count": results.len(),
            "tasks": results,
        })))
    }
}

pub fn task_registrations(tasks: Arc<Mutex<Vec<TaskEntry>>>) -> Vec<ToolRegistration> {
    vec![ToolRegistration::from(TaskResultViewerTool::new(tasks))]
}
