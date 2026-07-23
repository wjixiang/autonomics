//! Node abstractions and built-in implementations.
//!
//! [`meta`] defines the [`DagNode`] trait, [`NodePorts`], and [`NodeInput`] —
//! the contract every node fulfils. Concrete implementations live in
//! [`source`], [`sql_node`], and [`sink`].

pub mod echo_node;
pub mod ldsc_hsq;
pub mod ldsc_rg;
pub mod liability;
pub mod linear_regression;
pub mod meta;
pub mod mr;
pub mod sink;
pub mod source;
pub mod sql_node;
pub mod test_source;
pub mod viz;

pub use echo_node::{EchoNode, EchoNodeFactory, EchoNodeSpec};
pub use ldsc_hsq::{LdscHsqConfig, LdscHsqNode, LdscHsqNodeFactory};
pub use ldsc_rg::{LdscRgConfig, LdscRgNode, LdscRgNodeFactory};
pub use liability::{LiabilityConfig, LiabilityNode, LiabilityNodeFactory};
pub use linear_regression::{
    LinearRegressionNode, LinearRegressionNodeFactory, LinearRegressionNodeSpec,
};
pub use meta::{DEFAULT_PORT, DagNode, NodeId, NodeInput, NodePorts, Port};
pub use mr::{MrNode, MrNodeFactory, MrNodeSpec, MrParameters};
pub use sink::{Sink, SinkMode, SinkNode, SinkNodeFactory, SinkNodeSpec, WriteFormat};
pub use source::{FileFormat, Source, SourceNode, SourceNodeFactory, SourceNodeSpec};
pub use sql_node::{SqlNode, SqlNodeFactory, SqlNodeSpec};
pub use test_source::{TestSourceFactory, TestSourceNode, TestSourceSpec};
pub use viz::{VizNode, VizNodeFactory, VizNodeSpec};
