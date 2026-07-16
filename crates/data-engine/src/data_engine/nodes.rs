//! Node abstractions and built-in implementations.
//!
//! [`meta`] defines the [`DagNode`] trait, [`NodeMeta`], and [`NodeInput`] —
//! the contract every node fulfils. Concrete implementations live in
//! [`source`], [`sql_node`], and [`sink`].

pub mod cache_source;
pub mod ldsc_hsq;
pub mod linear_regression;
pub mod meta;
pub mod mock_node;
pub mod sink;
pub mod source;
pub mod sql_node;

pub use ldsc_hsq::{LdscHsqConfig, LdscHsqNode};
pub use linear_regression::LinearRegressionNode;
pub use meta::{DEFAULT_PORT, DagNode, NodeId, NodeInput, NodeMeta, Port};
pub use sink::{Sink, SinkMode, SinkNode, WriteFormat};
pub use source::{FileFormat, Source, SourceNode};
pub use sql_node::SqlNode;
