//! Node abstractions and built-in implementations.
//!
//! [`meta`] defines the [`DagNode`] trait, [`NodeMeta`], and [`NodeInput`] —
//! the contract every node fulfils. Concrete implementations live in
//! [`source`], [`sql_node`], and [`sink`].

pub mod meta;
pub mod mock_node;
pub mod preview_node;
pub mod sink;
pub mod source;
pub mod sql_node;

pub use meta::{DagNode, NodeId, NodeInput, NodeMeta};
pub use sink::{Sink, SinkNode, WriteFormat};
pub use source::{FileFormat, Source, SourceNode};
pub use sql_node::SqlNode;
