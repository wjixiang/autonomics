//! Runtime types for DAG execution.
//!
//! The actual scheduling logic lives in [`super::graph::DAG::run`]; this module
//! defines the types it produces and accepts.

use datafusion::common::HashMap;

use super::NodeId;
use super::error::DagError;

/// Per-node runtime lifecycle state tracked by the scheduler.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeStatus {
    #[default]
    Pending,
    Ready,
    Running,
    Success,
    Failed,
    /// Not run because an upstream predecessor failed.
    Skipped,
}

/// Scheduler tuning knobs.
#[derive(Clone)]
pub struct SchedulerConfig {
    /// Maximum number of nodes running concurrently (semaphore permits).
    pub max_concurrency: usize,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        let cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        Self {
            max_concurrency: cpus,
        }
    }
}

/// Result of a `DAG::run` invocation: the final status of every node and
/// whether the whole run succeeded.
#[derive(Debug)]
pub struct RunReport {
    pub statuses: HashMap<NodeId, RuntimeStatus>,
    pub errors: HashMap<NodeId, DagError>,
    pub ok: bool,
}

impl RunReport {
    pub fn status(&self, id: &str) -> Option<RuntimeStatus> {
        self.statuses.get(id).copied()
    }

    /// The error that failed `id`, if any.
    pub fn error(&self, id: &str) -> Option<&DagError> {
        self.errors.get(id)
    }
}

/// Target scope for a DAG run (reserved for future partial / sub-graph
/// execution).
pub enum DagRunTarget {
    StartWith,
    EndWith,
}
