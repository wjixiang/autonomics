//! Per-node metadata, ports, the typed input envelope, and the [`DagNode`] trait.
//!
//! Each node declares a set of **input ports** and **output ports** ([`Port`]). An edge
//! in the DAG connects exactly one upstream output port to one downstream input port and
//! carries exactly one [`DataFrame`]. At execution time the scheduler injects one
//! [`NodeInput`] per connected input port, tagged with the port name so the node knows
//! which slot each DataFrame belongs to.

use arrow_schema::SchemaRef;
use async_trait::async_trait;
use datafusion::prelude::DataFrame;

use crate::data_engine::dag::{DagError, graph::NamedDataFrames};

/// Unique identifier for a node in the DAG.
pub type NodeId = String;

/// The default port name used when a single-input/single-output node does not name its
/// ports explicitly.
pub const DEFAULT_PORT: &str = "default";

/// A named, optionally-typed socket on a node.
///
/// `schema` is `None` when the shape of the data is not known at graph-build time
/// (e.g. a source reading a file whose schema is discovered at runtime). When both ends
/// of an edge declare a schema, the engine validates compatibility in [`DAG::validate`].
#[derive(Debug, Clone)]
pub struct Port {
    pub name: String,
    pub schema: Option<SchemaRef>,
}

impl Port {
    /// An untyped port (schema discovered at runtime).
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            schema: None,
        }
    }

    /// A port with a known schema.
    pub fn typed(name: impl Into<String>, schema: SchemaRef) -> Self {
        Self {
            name: name.into(),
            schema: Some(schema),
        }
    }

    /// Convenience: the unnamed default port.
    pub fn default_port() -> Self {
        Self::new(DEFAULT_PORT)
    }
}

/// One upstream output injected into a node at execution time, one per connected input
/// port.
#[derive(Debug, Clone)]
pub struct NodeInput {
    /// The consuming node's INPUT port name this data arrived on (e.g. `"left"`,
    /// `"right"`, or `"default"`). The node looks up its inputs by this name.
    pub port: String,
    /// Globally-unique table name under which `data` is registered in the shared
    /// `SessionContext` (derived from the edge: `"{from_node}__{from_port}__{to_node}"`).
    /// Guaranteed not to collide with any other node's registration.
    pub df_name: String,
    pub data: DataFrame,
}

/// Static per-node metadata: identity plus declared input/output ports.
#[derive(Clone)]
pub struct NodeMeta {
    id: NodeId,
    input_ports: Vec<Port>,
    output_ports: Vec<Port>,
}

impl NodeMeta {
    /// A transform node with a single default input port and a single default output
    /// port — the backward-compatible shape for `SqlNode` and friends.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            input_ports: vec![Port::default_port()],
            output_ports: vec![Port::default_port()],
        }
    }

    /// A source node: no inputs, a single default output port.
    pub fn source(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            input_ports: vec![],
            output_ports: vec![Port::default_port()],
        }
    }

    /// A sink node: a single default input port, no outputs.
    pub fn sink(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            input_ports: vec![Port::default_port()],
            output_ports: vec![],
        }
    }

    /// Replace the input ports.
    pub fn with_inputs(mut self, ports: Vec<Port>) -> Self {
        self.input_ports = ports;
        self
    }

    /// Replace the output ports.
    pub fn with_outputs(mut self, ports: Vec<Port>) -> Self {
        self.output_ports = ports;
        self
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn input_ports(&self) -> &[Port] {
        &self.input_ports
    }

    pub fn output_ports(&self) -> &[Port] {
        &self.output_ports
    }

    /// Look up a declared input port by name.
    pub fn input_port(&self, name: &str) -> Option<&Port> {
        self.input_ports.iter().find(|p| p.name == name)
    }

    /// Look up a declared output port by name.
    pub fn output_port(&self, name: &str) -> Option<&Port> {
        self.output_ports.iter().find(|p| p.name == name)
    }
}

/// A single unit of work in the DAG.
///
/// `execute` receives one [`NodeInput`] per connected input port (the node identifies each
/// by its `port` field) and returns its outputs keyed by **output port name**, so the
/// engine can route each [`DataFrame`] through the correct outgoing edge.
///
/// - Stateless: all runtime state is managed by the DAG.
#[async_trait]
pub trait DagNode: Send + Sync {
    fn meta(&self) -> &NodeMeta;
    async fn execute(&mut self, inputs: &[NodeInput]) -> Result<NamedDataFrames, DagError>;
    fn clone_box(&self) -> Box<dyn DagNode>;

    /// Human-readable node kind (e.g. `"source"`, `"sql"`, `"sink"`).
    fn node_type(&self) -> &str;

    /// Downcast helper for concrete-type introspection (e.g. extracting
    /// sink-specific details at report time).
    fn as_any(&self) -> &dyn std::any::Any;
}

impl Clone for Box<dyn DagNode> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}
