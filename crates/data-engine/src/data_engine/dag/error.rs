//! DAG error types.
//!
//! [`DagError`] is the single error type surfaced by node bodies and the
//! scheduler. Structural failures (cycles, unknown / duplicate node ids,
//! scheduler invariant violations) get their own variants so callers can
//! switch on them; DataFusion errors propagate directly.

use thiserror::Error;

pub type Result<T> = std::result::Result<T, DagError>;

#[derive(Debug, Error)]
pub enum DagError {
    /// Propagates any [`datafusion::error::DataFusionError`] that escapes a
    /// node body (e.g. register_table, sql, read_csv).
    #[error("datafusion: {0}")]
    DataFusion(#[from] datafusion::error::DataFusionError),

    /// The graph contains a cycle; carries a representation of the cycle
    /// path (e.g. `A → B → C → A`) for diagnostics.
    #[error("cycle detected: {0}")]
    Cycle(String),

    /// An edge referenced a node id that was never added.
    #[error("unknown node id: {0}")]
    UnknownNode(String),

    /// `add_node` was called twice with the same id.
    #[error("duplicate node id: {0}")]
    DuplicateNode(String),

    /// A scheduler invariant was violated (e.g. a job result arrived for a node
    /// the scheduler did not dispatch).
    #[error("scheduler: {0}")]
    Schedule(String),

    #[error("node `{node_type}` failed: {msg}")]
    NodeError { node_type: String, msg: String },
}
