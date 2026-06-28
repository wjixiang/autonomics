mod view_task_results;
mod view_task_status;

use std::sync::Arc;
use tokio::sync::RwLock;

use crate::tools::task_runtime::TaskEntry;
use crate::tools::ToolRegistration;

pub use view_task_results::{TaskResultViewerTool, ViewTaskResultsInput};
pub use view_task_status::{TaskStatusViewerTool, ViewTaskStatusInput};

pub fn task_registrations(tasks: Arc<RwLock<Vec<TaskEntry>>>) -> Vec<ToolRegistration> {
    vec![
        ToolRegistration::from(TaskResultViewerTool::new(tasks.clone())),
        ToolRegistration::from(TaskStatusViewerTool::new(tasks)),
    ]
}
