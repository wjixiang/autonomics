//! Per-node metadata, the typed input envelope, and the [`DagNode`] trait.
//!
//! Nodes receive their predecessor outputs as a slice of [`NodeInput`], each
//! carrying a `port` (the table / view name under which the upstream output is
//! registered, e.g. for [`crate::data_engine::nodes::SqlNode`]) and the
//! [`DataFrame`] itself. The trait is `Send` so node payloads can be moved into
//! spawned scheduler tasks.

use async_trait::async_trait;
use datafusion::prelude::DataFrame;

use crate::data_engine::dag::DagError;

/// Unique identifier for a node in the DAG.
pub type NodeId = String;

/// One upstream output injected into a node at execution time.
#[derive(Debug, Clone)]
pub struct NodeInput {
    /// Name under which `data` is registered for the consuming node
    /// (e.g. the table name a `SqlNode` references). Positional default is
    /// `"src"`, `"src_2"`, … assigned by the scheduler.
    pub port: String,
    pub data: DataFrame,
}

/// Static per-node metadata.
#[derive(Clone)]
pub struct NodeMeta {
    id: NodeId,
}

impl NodeMeta {
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into() }
    }

    pub fn id(&self) -> &str {
        &self.id
    }
}

/// A single unit of work in the DAG.
///
/// `execute` receives the outputs of all predecessor nodes (in declared edge
/// order) and returns its own outputs, which are then fanned out to successors.
#[async_trait]
pub trait DagNode: Send + Sync {
    fn meta(&self) -> &NodeMeta;
    /// Input data injected by the scheduler when the node runs.
    async fn execute(&mut self, inputs: &[NodeInput]) -> Result<Vec<DataFrame>, DagError>;
    fn clone_box(&self) -> Box<dyn DagNode>;
}

impl Clone for Box<dyn DagNode> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}
