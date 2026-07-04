//! DAG error types.
//!
//! [`DagError`] is the single error type surfaced by node bodies and the
//! scheduler. Concrete node errors are unified into [`DagError::ExecutionError`]
//! via a stable classification `kind` tag, while structural failures (cycles,
//! unknown / duplicate node ids, scheduler invariant violations) get their own
//! variants so callers can switch on them.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DagError {
    /// Wraps any node execution failure together with a stable classification
    /// tag. Built via [`DagError::execution`]; each node error type implements
    /// `From<Self> for DagError` so `?` propagates inside `execute` bodies.
    #[error("[{kind}] {source}")]
    ExecutionError {
        kind: &'static str,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },

    /// Propagates any [`datafusion::error::DataFusionError`] that escapes a
    /// node body (e.g. register_table, sql, read_csv).
    #[error("datafusion: {0}")]
    DataFusion(#[from] datafusion::error::DataFusionError),

    /// The graph contains a cycle; carries a comma-separated list of the
    /// offending nodes for diagnostics.
    #[error("cycle detected involving nodes: {0}")]
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
}

impl DagError {
    /// Wrap any node error into [`DagError::ExecutionError`] with a stable
    /// classification tag.
    pub fn execution(
        kind: &'static str,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::ExecutionError {
            kind,
            source: Box::new(source),
        }
    }

    /// Stable classification tag for logging / UI / retry policies.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::ExecutionError { kind, .. } => kind,
            Self::DataFusion(_) => "datafusion",
            Self::Cycle(_) => "dag.cycle",
            Self::UnknownNode(_) => "dag.unknown_node",
            Self::DuplicateNode(_) => "dag.duplicate_node",
            Self::Schedule(_) => "dag.schedule",
        }
    }
}
