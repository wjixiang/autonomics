use async_trait::async_trait;
use datafusion::common::HashMap;
use datafusion::common::HashSet;
use thiserror::Error;

use crate::dataset::DatasetRef;

type NodeId = String;

#[derive(Debug, Clone, Copy)]
pub enum DependencyKind {
    OneToOne,
    Shuffle,
}

pub struct Edge {
    pub from: NodeId,
    pub to: NodeId,
    pub kind: DependencyKind,
}

pub struct DAG {
    pub nodes: HashMap<NodeId, Box<dyn DagNode>>,
    pub edges: Vec<Edge>,
}

pub struct DagScheduler {}

#[derive(Debug, Error)]
pub enum DagError {
    /// Wraps any node execution failure together with a stable classification
    /// tag. Built via [`DagError::execution`]; each node error type implements
    /// `From<Self> for DagError` so `?` propagates inside `execute` bodies
    /// without touching this enum.
    #[error("[{kind}] {source}")]
    ExecutionError {
        kind: &'static str,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },

    /// Fallback for dataset errors that escape a node without being wrapped
    /// (e.g. returned directly from a helper that does not own a node error).
    #[error(transparent)]
    Dataset(#[from] crate::DatasetError),
}

impl DagError {
    /// Wrap any node error into [`DagError::ExecutionError`] with a stable
    /// classification tag. Use this when adding a new node type and defining
    /// its `From<...> for DagError` impl.
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
            Self::Dataset(_) => "dataset",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub enum DagNodeStatus {
    #[default]
    Idle,
    Success,
    Running,
    Failed,
}

pub struct NodeMeta {
    id: String,
    name: String,
    status: DagNodeStatus,
}

impl NodeMeta {
    pub fn new(id: String, name: String, status: DagNodeStatus) -> Self {
        Self { id, name, status }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn status(&self) -> DagNodeStatus {
        self.status.clone()
    }
}

#[async_trait]
pub trait DagNode {
    fn meta(&self) -> &NodeMeta;
    /// Input data injected by system when executed
    async fn execute(&mut self, inputs: &[DatasetRef]) -> Result<(), DagError>;
    // TODO: Remove this field, get output ids directly from exeucte result (Change to use Arc<Dataset> is better)
    fn get_output_ids(&self) -> Result<&HashSet<String>, DagError>;
}
