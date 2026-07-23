//! Runtime types for DAG execution.
//!
//! The actual scheduling logic lives in [`super::graph::DAG::run`]; this module
//! defines the types it produces and accepts.

use datafusion::common::HashMap;
use serde::Serialize;

use super::NodeId;
use super::error::DagError;

/// Schema map serializable via serde (uses `std::collections::HashMap` since
/// `datafusion::common::HashMap` is `hashbrown` and may not impl `Serialize`).
type SchemaMap = std::collections::HashMap<String, String>;

/// Per-node runtime lifecycle state tracked by the scheduler.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
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
    /// When `true`, [`NodeReport::output_rows`] is populated by forcing every
    /// successful node's `DataFrame` to be collected (i.e. `SELECT COUNT(*)`
    /// over the LogicalPlan). This is an **eager** operation — for a source
    /// node reading `.vcf.gz` or similar, it triggers full decompression and
    /// parsing of every record, dominating the run cost.
    ///
    /// Defaults to `false` so that `run_dag` remains a "logical only" call:
    /// `SourceNode`/`SqlNode` execute lazily, the report reflects timings and
    /// schema without doing the I/O. Enable explicitly when downstream tooling
    /// (agents, dashboards, callers) needs row counts.
    pub compute_row_counts: bool,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        let cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        Self {
            max_concurrency: cpus,
            compute_row_counts: false,
        }
    }
}

/// A serializable error summary extracted from [`DagError`].
///
/// Carries only the agent-relevant fields (kind + message) rather than the
/// full structured variant, because [`datafusion::error::DataFusionError`]
/// does not implement `Serialize`.
#[derive(Debug, Clone, Serialize)]
pub struct DagErrorReport {
    /// Error variant category (e.g. `"datafusion"`, `"schema_mismatch"`,
    /// `"node_error"`).
    pub kind: String,
    /// Full human-readable error message.
    pub message: String,
}

/// Per-node execution summary produced by [`super::graph::DAG::run`].
///
/// Contains everything an agent needs to understand what each node did
/// without a follow-up `get_output` call — type, output shape, timing,
/// sink destination, and error/skip details.
#[derive(Debug, Serialize)]
pub struct NodeReport {
    pub id: String,
    pub status: RuntimeStatus,
    pub node_type: String,
    /// Output column schema: `{ column_name: data_type }`.
    pub output_schema: Option<SchemaMap>,
    /// Row count of the primary output DataFrame.
    pub output_rows: Option<usize>,
    /// Milliseconds spent in `execute()`.
    pub elapsed_ms: Option<u64>,

    /// For `VizNode` (and future artifact-producing nodes): the path of the
    /// rendered/produced artifact (e.g. a PNG).
    pub artifact_path: Option<String>,
=======
    /// For `sink_file` nodes: the file path data was written to.
    pub file_path: Option<String>,
>>>>>>> main
    /// Structured error info when `status` is `Failed`.
    pub error: Option<DagErrorReport>,
    /// For `Skipped` nodes: the id of the root-cause failed node.
    pub skipped_because: Option<String>,
}

/// Result of a `DAG::run` invocation: the final status of every node and
/// whether the whole run succeeded.
#[derive(Debug)]
pub struct RunReport {
    pub ok: bool,
    /// Rich per-node reports (serializable, agent-friendly).
    pub nodes: Vec<NodeReport>,
    /// Flat status map kept for backward-compatible programmatic access.
    pub statuses: HashMap<NodeId, RuntimeStatus>,
    /// Per-node errors (only populated for `Failed` nodes).
    pub errors: HashMap<NodeId, DagError>,
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
