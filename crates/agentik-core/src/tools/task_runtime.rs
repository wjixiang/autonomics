use crate::agent::InternalEvent;
use agentik_sdk::ToolResult;
use std::time::Duration;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::tools::error::ToolError;

pub type TaskId = String;

#[derive(Clone)]
pub enum RunMode {
    Fg,
    Bg,
}

/// Lifecycle of a spawned tool invocation.
#[derive(Clone)]
pub enum TaskStatus {
    Running,
    Done(ToolResult),
    Failed(ToolError),
}

/// What `wait()` returns: result available or still running.
pub enum WaitResultKind {
    Done {
        result: ToolResult,
        run_mode: RunMode,
    },
    StillRunning(TaskId),
    Failed(ToolResult),
}

pub struct WaitResult {
    pub inner: WaitResultKind,
    read_tx: watch::Sender<bool>,
}

impl From<WaitResult> for ToolResult {
    fn from(value: WaitResult) -> Self {
        match value.inner {
            WaitResultKind::StillRunning(id) => ToolResult::from_pending_task(&id),
            WaitResultKind::Failed(tool_result) => {
                value.read_tx.send(true).ok();
                tool_result
            }
            WaitResultKind::Done { result, run_mode } => match run_mode {
                RunMode::Fg => {
                    value.read_tx.send(true).ok();
                    result
                }
                RunMode::Bg => ToolResult::task_finish_notification(result.tool_use_id.as_str()),
            },
        }
    }
}

/// Sender type for background task completion notifications.
pub type BgTaskNotifyTx = tokio::sync::mpsc::UnboundedSender<InternalEvent>;

/// A single tool invocation tracked by [`Toolset`](super::toolset::Toolset).
///
/// Status is self-managed via a `watch` channel. A monitor task owns the
/// `JoinHandle` and updates the channel when the tool completes, so callers
/// can retrieve the result at any time via [`status`](Self::status) or
/// [`changed`](Self::changed).
///
/// When a background task completes, the monitor task sends
/// [`InternalEvent::BgTaskComplete`] through the optional `notify_tx`,
/// allowing the agent to wake up without polling.
pub struct TaskEntry {
    id: TaskId,
    /// The tool's display name (e.g. "run_bash"), distinct from the task id.
    name: String,
    status: watch::Receiver<TaskStatus>,
    cancel_token: CancellationToken,
    block_secs: u64,
    read: watch::Receiver<bool>,
    read_tx: watch::Sender<bool>,
    run_mode: RunMode,
    /// Optional sender to notify the agent when a background task completes.
    notify_tx: Option<BgTaskNotifyTx>,
    /// Accumulated output from the tool execution, readable at any time.
    output: watch::Receiver<String>,
    output_tx: watch::Sender<String>,
}

impl TaskEntry {
    /// Spawn a monitor task that awaits the `JoinHandle` and updates the
    /// status channel on completion. The handle is consumed here; callers
    /// read results exclusively through the watch channel.
    pub fn new(
        id: TaskId,
        name: String,
        handle: JoinHandle<Result<ToolResult, ToolError>>,
        cancel_token: CancellationToken,
        block_secs: u64,
    ) -> Self {
        Self::with_notify(id, name, handle, cancel_token, block_secs, None)
    }

    /// Like [`new`](Self::new) but also notifies the agent via `notify_tx`
    /// when a background task completes.
    pub fn with_notify(
        id: TaskId,
        name: String,
        handle: JoinHandle<Result<ToolResult, ToolError>>,
        cancel_token: CancellationToken,
        block_secs: u64,
        notify_tx: Option<BgTaskNotifyTx>,
    ) -> Self {
        let (status_tx, status) = watch::channel(TaskStatus::Running);
        let (read_tx, read) = watch::channel(false);
        let (output_tx, output) = watch::channel(String::new());

        let tx = status_tx.clone();
        let bg_notify = notify_tx.clone();
        let out = output_tx.clone();
        tokio::spawn(async move {
            match handle.await {
                Ok(Ok(tool_result)) => {
                    let _ = out.send(tool_result.text_content());
                    tx.send(TaskStatus::Done(tool_result)).ok();
                }
                Ok(Err(e)) => {
                    let _ = out.send(e.to_string());
                    tx.send(TaskStatus::Failed(e)).ok();
                }
                Err(join_err) => {
                    let msg = format!("task panicked: {join_err}");
                    let _ = out.send(msg.clone());
                    tx.send(TaskStatus::Failed(ToolError::ExecutionFailed {
                        source: Box::new(join_err),
                    }))
                    .ok();
                }
            }
            // Notify the agent's event loop that a background task finished.
            if let Some(notify) = bg_notify {
                let _ = notify.send(InternalEvent::BgTaskComplete);
            }
        });

        Self {
            id,
            name,
            status,
            cancel_token,
            block_secs,
            read,
            read_tx,
            run_mode: RunMode::Fg,
            notify_tx,
            output,
            output_tx,
        }
    }

    /// Return the task identifier.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Return the tool's display name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Non-blocking read of current status.
    ///
    /// When the task completes, returns `TaskStatus::Done(result)` —
    /// the result is embedded in the status itself.
    pub fn status(&self) -> TaskStatus {
        self.status.borrow().clone()
    }

    pub fn run_mode(&self) -> &RunMode {
        &self.run_mode
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
                    TaskStatus::Done(result) => WaitResult { inner: WaitResultKind::Done { result, run_mode: RunMode::Fg }, read_tx: self.read_tx.clone() },
                    TaskStatus::Failed(err) => WaitResult { inner: WaitResultKind::Failed(ToolResult::error(err.to_string()).with_id(&self.id)), read_tx: self.read_tx.clone() },
                    // WARN: this variant will never be reached
                    TaskStatus::Running =>
                       WaitResult { inner: WaitResultKind::StillRunning(self.id.clone()), read_tx: self.read_tx.clone() },
                }
            }
            _ = tokio::time::sleep(Duration::from_secs(self.block_secs)) => {
                // Sync phase expired, task continues async
                self.run_mode = RunMode::Bg;
                WaitResult { inner: WaitResultKind::StillRunning(self.id.clone()), read_tx: self.read_tx.clone() }
            }
        }
    }

    pub fn is_read(&self) -> bool {
        *self.read.borrow()
    }

    /// Mark this task's result as consumed.
    pub fn mark_read(&self) {
        self.read_tx.send(true).ok();
    }

    /// Return a clone of the background task notification sender.
    pub fn notify_tx(&self) -> Option<BgTaskNotifyTx> {
        self.notify_tx.clone()
    }

    /// Non-blocking read of accumulated output so far.
    pub fn output(&self) -> String {
        self.output.borrow().clone()
    }

    /// Return a clone of the output sender, so the spawned task can
    /// write incremental output during execution.
    pub fn output_tx(&self) -> watch::Sender<String> {
        self.output_tx.clone()
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[tokio::test]
    async fn test_task_two_phase() {
        let mut task = TaskEntry::new(
            "test-task-1".into(),
            "test_tool".into(),
            tokio::spawn(async {
                tokio::time::sleep(Duration::from_secs(5)).await;
                Ok(ToolResult::success("done"))
            }),
            CancellationToken::new(),
            1,
        );

        // Phase 1 (sync): task takes 5s but block_secs=1, should StillRunning
        let result = task.wait().await;
        assert!(matches!(result.inner, WaitResultKind::StillRunning(_)));

        // Phase 2 (async): task is still running, wait for it to actually finish
        assert!(matches!(task.status(), TaskStatus::Running));
        task.changed().await;
        assert!(matches!(task.status(), TaskStatus::Done(_)));
    }
}
