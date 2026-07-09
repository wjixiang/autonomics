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

    /// An edge referenced an input/output port that the node did not declare.
    /// `direction` is `"input"` or `"output"`.
    #[error("node `{node}` has no {direction} port `{port}`")]
    PortNotFound {
        node: String,
        port: String,
        direction: &'static str,
    },

    /// A declared input port has no incoming edge (every input port must be
    /// connected).
    #[error("input port `{port}` on node `{node}` is not connected")]
    PortDisconnected { node: String, port: String },

    /// More than one edge connects to the same input port (strict 1:1).
    #[error("input port `{port}` on node `{node}` has multiple incoming edges")]
    PortOverconnected { node: String, port: String },

    /// `add_edge` (the default-port form) was used on a node with more than one
    /// relevant port, so the port is ambiguous.
    #[error("node `{node}` has multiple ports; use `add_edge_port` to specify which")]
    AmbiguousPort { node: String },

    /// Connected ports declare incompatible schemas.
    #[error("schema mismatch on edge {from_node}.{from_port} -> {to_node}.{to_port}: {reason}")]
    SchemaMismatch {
        from_node: String,
        from_port: String,
        to_node: String,
        to_port: String,
        reason: String,
    },

    /// A scheduler invariant was violated (e.g. a job result arrived for a node
    /// the scheduler did not dispatch).
    #[error("scheduler: {0}")]
    Schedule(String),

    #[error("node `{node_type}` failed: {msg}")]
    NodeError { node_type: String, msg: String },
}
