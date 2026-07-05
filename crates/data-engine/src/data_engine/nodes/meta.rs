//! Per-node metadata, the typed input envelope, and the [`DagNode`] trait.
//!
//! Nodes receive their predecessor outputs as a slice of [`NodeInput`], each
//! carrying a `port` (the table / view name under which the upstream output is
//! registered, e.g. for [`crate::data_engine::nodes::SqlNode`]) and the
//! [`DataFrame`] itself. The trait is `Send` so node payloads can be moved into
//! spawned scheduler tasks.

use std::sync::Arc;

use async_trait::async_trait;
use datafusion::prelude::{DataFrame, SessionContext};

use crate::data_engine::dag::DagError;

/// Unique identifier for a node in the DAG.
pub type NodeId = String;

/// Static, declared-at-construction metadata status. Distinct from the runtime
/// status the scheduler tracks while a run is in flight (see
/// [`crate::data_engine::dag::RuntimeStatus`]).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum DagNodeStatus {
    #[default]
    Idle,
    Success,
    Running,
    Failed,
}

/// One upstream output injected into a node at execution time.
#[derive(Debug, Clone)]
pub struct NodeInput {
    /// Name under which `data` is registered for the consuming node
    /// (e.g. the table name a `SqlNode` references). Positional default is
    /// `"src"`, `"src_2"`, … assigned by the scheduler.
    pub port: String,
    pub data: DataFrame,
}

/// Static per-node metadata. The shared [`SessionContext`] is injected here by
/// [`crate::data_engine::DataEngine::node_meta`] so every node reads through the
/// same engine context (registered object stores, catalogs, …).
pub struct NodeMeta {
    id: NodeId,
    status: DagNodeStatus,
    ctx: Arc<SessionContext>,
}

impl NodeMeta {
    pub fn new(id: impl Into<String>, ctx: Arc<SessionContext>) -> Self {
        Self {
            id: id.into(),
            status: DagNodeStatus::default(),
            ctx,
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    /// Status borrowed by reference — caller must not outlive the meta node.
    pub fn status(&self) -> &DagNodeStatus {
        &self.status
    }

    /// Returns an `Arc::clone` of the shared SessionContext.
    pub fn ctx(&self) -> Arc<SessionContext> {
        self.ctx.clone()
    }
}

/// A single unit of work in the DAG.
///
/// `execute` receives the outputs of all predecessor nodes (in declared edge
/// order) and returns its own outputs, which are then fanned out to successors.
#[async_trait]
pub trait DagNode: Send {
    fn meta(&self) -> &NodeMeta;
    /// Input data injected by the scheduler when the node runs.
    async fn execute(&mut self, inputs: &[NodeInput]) -> Result<Vec<DataFrame>, DagError>;
}
