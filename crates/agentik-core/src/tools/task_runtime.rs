use agentik_sdk::ToolResult;
use std::time::Duration;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::tools::error::ToolError;

pub type TaskId = String;

#[derive(Clone)]
pub enum RunningStatus {
    Fg,
    Bg,
}

/// Lifecycle of a spawned tool invocation.
#[derive(Clone)]
pub enum TaskStatus {
    Running(RunningStatus),
    Done(ToolResult),
    Failed(ToolError),
}

/// What `wait()` returns: result available or still running.
pub enum WaitResult {
    Done(ToolResult),
    StillRunning(TaskId),
    Failed(ToolResult),
}

// impl From<WaitResult> for ToolResult {
//     fn from(value: WaitResult) -> ToolResult {
//         match value {
//             WaitResult::Done(tool_result) => tool_result,
//             WaitResult::StillRunning(id) => ToolResult::from_backend_task(&id),
//             WaitResult::Failed(tool_result) => tool_result,
//         }
//     }
// }

/// A single tool invocation tracked by [`Toolset`](super::toolset::Toolset).
///
/// Status is self-managed via a `watch` channel. A monitor task owns the
/// `JoinHandle` and updates the channel when the tool completes, so callers
/// can retrieve the result at any time via [`status`](Self::status) or
/// [`changed`](Self::changed).
pub struct TaskEntry {
    id: TaskId,
    status: watch::Receiver<TaskStatus>,
    status_tx: watch::Sender<TaskStatus>,
    cancel_token: CancellationToken,
    block_secs: u64,
    read: bool,
}

impl TaskEntry {
    /// Spawn a monitor task that awaits the `JoinHandle` and updates the
    /// status channel on completion. The handle is consumed here; callers
    /// read results exclusively through the watch channel.
    pub fn new(
        id: TaskId,
        handle: JoinHandle<Result<ToolResult, ToolError>>,
        cancel_token: CancellationToken,
        block_secs: u64,
    ) -> Self {
        let (status_tx, status) = watch::channel(TaskStatus::Running(RunningStatus::Fg));

        let tx = status_tx.clone();
        tokio::spawn(async move {
            match handle.await {
                Ok(Ok(tool_result)) => {
                    tx.send(TaskStatus::Done(tool_result)).ok();
                }
                Ok(Err(e)) => {
                    tx.send(TaskStatus::Failed(e)).ok();
                }
                Err(join_err) => {
                    tx.send(TaskStatus::Failed(ToolError::ExecutionFailed {
                        source: Box::new(join_err),
                    }))
                    .ok();
                }
            }
        });

        Self {
            id,
            status,
            status_tx,
            cancel_token,
            block_secs,
            read: false,
        }
    }

    /// Return the task identifier.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Non-blocking read of current status.
    ///
    /// When the task completes, returns `TaskStatus::Done(result)` —
    /// the result is embedded in the status itself.
    pub fn status(&self) -> TaskStatus {
        self.status.borrow().clone()
    }

    /// Wait for the next status change (e.g. `Running` → `Done`).
    pub async fn changed(&mut self) {
        self.status.changed().await.ok();
    }

    /// Signal cancellation to the running task.
    pub fn cancel(&self) {
        self.cancel_token.cancel();
    }

    /// Wait up to `block_secs` for the task to complete.
    ///
    /// - `Done(result)` — task finished within the sync window
    /// - `StillRunning` — sync phase expired, task continues async;
    ///   poll [`status()`](Self::status) or [`changed()`](Self::changed) later
    /// - `Failed(msg)` — task errored
    pub async fn wait(&mut self) -> WaitResult {
        tokio::select! {
            _ = self.status.changed() => {
                match self.status.borrow().clone() {
                    TaskStatus::Done(result) => WaitResult::Done(result),
                    TaskStatus::Failed(err) => WaitResult::Failed(ToolResult::error(err.to_string()).with_id(&self.id)),
                    TaskStatus::Running(st) => {
                        match st {
                            RunningStatus::Fg => unreachable!(), // Never will change into Fg status
                            RunningStatus::Bg =>
                       WaitResult::StillRunning(self.id.clone()),
                        }
                    },
                }
            }
            _ = tokio::time::sleep(Duration::from_secs(self.block_secs)) => {
                // Sync phase expired, task continues async
                self.status_tx.send(TaskStatus::Running(RunningStatus::Bg)).ok();
                WaitResult::StillRunning(self.id.clone())
            }
        }
    }
}
