//! DAG workflow engine: graph model, and async scheduler.
//!
//! Node abstractions ([`DagNode`] trait, [`NodeMeta`], [`NodeInput`]) live in
//! [`crate::data_engine::nodes`] and are re-exported here for convenience so
//! existing `use crate::data_engine::dag::{DagNode, ...}` paths keep working.
//!
//! - [`graph`] — the [`DAG`] struct, edges, topological sort, cycle detection.
//! - [`error`] — [`DagError`].
//! - [`runtime`] — the async readiness scheduler and [`RunReport`].

pub mod error;
pub mod graph;
pub mod runtime;
pub mod utils;

// Re-export node abstractions from the nodes module for backward compatibility
// and so that dag internals (graph.rs, runtime.rs) can use `super::DagNode` etc.
pub use super::nodes::{DagNode, NodeId, NodeInput, NodeMeta};

pub use error::DagError;
pub use graph::{DAG, DependencyKind};
pub use runtime::{RunReport, RuntimeStatus, SchedulerConfig};
