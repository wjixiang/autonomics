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

    #[error("fail to resolve node_id to node_idx")]
    CannotResolveNodeIdx { node_id: String },

    /// `add_node` was called twice with the same id.
    #[error("duplicate node id: {0}")]
    DuplicateNode(String),

    /// An edge referenced an input/output port that the node did not declare.
    /// `direction` is `"input"` or `"output"`.
    #[error("node `{node}` has no {direction} port `{port}`")]
    PortNotFound {
        node: String,
        port: u8,
        direction: &'static str,
    },

    /// A declared input port has no incoming edge (every input port must be
    /// connected).
    #[error("input port `{port}` on node `{node}` is not connected")]
    PortDisconnected { node: String, port: u8 },

    /// More than one edge connects to the same input port (strict 1:1).
    #[error("input port `{port}` on node `{node}` has multiple incoming edges")]
    PortOverconnected { node: String, port: u8 },

    /// Connected ports declare incompatible schemas.
    #[error("schema mismatch on edge {from_node}.{from_port} -> {to_node}.{to_port}: {reason}")]
    SchemaMismatch {
        from_node: String,
        from_port: u8,
        to_node: String,
        to_port: u8,
        reason: String,
    },

    /// A scheduler invariant was violated (e.g. a job result arrived for a node
    /// the scheduler did not dispatch).
    #[error("scheduler: {0}")]
    Schedule(String),

    #[error("node `{node_type}` failed: {msg}")]
    NodeError { node_type: String, msg: String },

    /// No edge exists between the given node–port pair.
    #[error("no edge from `{from}.{from_port}` to `{to}.{to_port}`")]
    EdgeNotFound {
        from: String,
        from_port: u8,
        to: String,
        to_port: u8,
    },
}

impl DagError {
    /// Extract a serializable error summary for agent-facing reports.
    pub fn to_report(&self) -> super::runtime::DagErrorReport {
        match self {
            Self::DataFusion(e) => super::runtime::DagErrorReport {
                kind: "datafusion".into(),
                message: e.to_string(),
            },
            Self::Cycle(s) => super::runtime::DagErrorReport {
                kind: "cycle".into(),
                message: s.clone(),
            },
            Self::UnknownNode(s) => super::runtime::DagErrorReport {
                kind: "unknown_node".into(),
                message: s.clone(),
            },
            Self::CannotResolveNodeIdx { node_id } => super::runtime::DagErrorReport {
                kind: "error_resolve_idx".into(),
                message: format!(
                    "Error occured when resolve node_id '{node_id}' into graph idx. This may caused by Node didn't registered in graph set properly."
                ),
            },
            Self::DuplicateNode(s) => super::runtime::DagErrorReport {
                kind: "duplicate_node".into(),
                message: s.clone(),
            },
            Self::PortNotFound {
                node,
                port,
                direction,
            } => super::runtime::DagErrorReport {
                kind: "port_not_found".into(),
                message: format!("node `{node}` has no {direction} port `{port}`"),
            },
            Self::PortDisconnected { node, port } => super::runtime::DagErrorReport {
                kind: "port_disconnected".into(),
                message: format!("input port `{port}` on node `{node}` is not connected"),
            },
            Self::PortOverconnected { node, port } => super::runtime::DagErrorReport {
                kind: "port_overconnected".into(),
                message: format!(
                    "input port `{port}` on node `{node}` has multiple incoming edges"
                ),
            },
            Self::SchemaMismatch {
                from_node,
                from_port,
                to_node,
                to_port,
                reason,
            } => super::runtime::DagErrorReport {
                kind: "schema_mismatch".into(),
                message: format!("edge {from_node}.{from_port} -> {to_node}.{to_port}: {reason}"),
            },
            Self::Schedule(s) => super::runtime::DagErrorReport {
                kind: "schedule".into(),
                message: s.clone(),
            },
            Self::NodeError { node_type, msg } => super::runtime::DagErrorReport {
                kind: "node_error".into(),
                message: format!("node `{node_type}` failed: {msg}"),
            },
            Self::EdgeNotFound {
                from,
                from_port,
                to,
                to_port,
            } => super::runtime::DagErrorReport {
                kind: "edge_not_found".into(),
                message: format!("no edge from `{from}.{from_port}` to `{to}.{to_port}`"),
            },
        }
    }
}
