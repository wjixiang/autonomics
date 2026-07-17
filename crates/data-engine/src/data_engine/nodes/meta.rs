//! Per-node metadata, ports, the typed input envelope, and the [`DagNode`] trait.
//!
//! Each node declares a set of **input ports** and **output ports** ([`Port`]). An edge
//! in the DAG connects exactly one upstream output port to one downstream input port and
//! carries exactly one [`DataFrame`]. At execution time the scheduler injects one
//! [`NodeInput`] per connected input port, tagged with the port name so the node knows
//! which slot each DataFrame belongs to.

use arrow_schema::SchemaRef;
use async_trait::async_trait;
use datafusion::{common::HashMap, prelude::DataFrame};

use crate::data_engine::dag::{DagError, graph::PortOutputs};

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
    pub index: u8,
    pub schema: Option<SchemaRef>,
}

impl Port {
    /// An untyped port (schema discovered at runtime).
    pub fn new(index: u8, schema: Option<SchemaRef>) -> Self {
        Self { index, schema }
    }

    /// A port with a known schema.
    pub fn typed(index: u8, schema: SchemaRef) -> Self {
        Self {
            index,
            schema: Some(schema),
        }
    }

    /// Convenience: the unnamed default port.
    #[deprecated]
    pub fn default_port() -> Self {
        Self::new(0, None)
    }

    pub fn get_code(&self) -> String {
        format!("port_{0}", self.index.clone())
    }
}

#[derive(Clone)]
pub struct Ports {
    ports: HashMap<u8, Port>,
    is_fixed: bool,
}
impl Default for Ports {
    fn default() -> Self {
        Self {
            ports: Default::default(),
            is_fixed: true,
        }
    }
}

impl Ports {
    pub fn add_port(&mut self, schema: Option<SchemaRef>) {
        let length = self.ports.len();
        self.ports
            .insert(length as u8, Port::new(length as u8, schema));
    }

    pub fn get(&self, index: u8) -> Option<&Port> {
        self.ports.get(&index)
    }

    pub fn len(&self) -> usize {
        self.ports.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ports.is_empty()
    }

    /// Iterate over ports in index order.
    pub fn iter(&self) -> impl Iterator<Item = &Port> {
        let mut keys: Vec<_> = self.ports.keys().copied().collect();
        keys.sort();
        keys.into_iter().filter_map(move |k| self.ports.get(&k))
    }
}

impl From<Vec<Port>> for Ports {
    fn from(ports: Vec<Port>) -> Self {
        let map = ports.into_iter().map(|p| (p.index, p)).collect();
        Self {
            ports: map,
            is_fixed: true,
        }
    }
}

/// One upstream output injected into a node at execution time, one per connected input
/// port.
#[derive(Debug, Clone)]
pub struct NodeInput {
    /// The consuming node's INPUT port name this data arrived on (e.g. `"left"`,
    /// `"right"`, or `"default"`). The node looks up its inputs by this name.
    pub port: u8,
    /// Globally-unique table name under which `data` is registered in the shared
    /// `SessionContext` (derived from the edge: `"{from_node}__{from_port}__{to_node}"`).
    /// Guaranteed not to collide with any other node's registration.
    ///
    /// TODO: Need to remove this field, DAG node should use auto-generated port index to reference
    /// instead of naming dataframe explicitly.
    // pub df_name: String,
    pub data: DataFrame,
}

/// Static per-node metadata: identity plus declared input/output ports.
#[derive(Clone)]
pub struct NodeMeta {
    id: NodeId,
    input_ports: Ports,
    output_ports: Ports,
}

impl NodeMeta {
    /// A transform node with a single default input port and a single default output
    /// port — the backward-compatible shape for `SqlNode` and friends.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            input_ports: Ports::default(),
            output_ports: Ports::default(),
        }
    }

    /// A source node: no inputs, a single default output port.
    pub fn source(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            input_ports: Ports::default(),
            output_ports: Ports::default(),
        }
        .add_output_port(None)
    }

    /// A sink node: a single default input port, no outputs.
    pub fn sink(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            input_ports: Ports::default(),
            output_ports: Ports::default(),
        }
        .add_input_port(None)
    }

    pub fn set_fixed_input(mut self, is_fixed: bool) -> Self {
        self.input_ports.is_fixed = is_fixed;
        self
    }

    pub fn add_output_port(mut self, schema: Option<SchemaRef>) -> Self {
        self.output_ports.add_port(schema);
        self
    }

    pub fn add_input_port(mut self, schema: Option<SchemaRef>) -> Self {
        self.input_ports.add_port(schema);
        self
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn input_ports(&self) -> &Ports {
        &self.input_ports
    }

    pub fn output_ports(&self) -> &Ports {
        &self.output_ports
    }

    /// Look up a declared input port by index.
    pub fn input_port(&self, index: u8) -> Option<&Port> {
        self.input_ports.get(index)
    }

    /// Look up a declared output port by index.
    pub fn output_port(&self, index: u8) -> Option<&Port> {
        self.output_ports.get(index)
    }

    pub fn is_fixed_input(&self) -> bool {
        self.input_ports.is_fixed
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
    async fn execute(&mut self, inputs: &[NodeInput]) -> Result<PortOutputs, DagError>;
    fn clone_box(&self) -> Box<dyn DagNode>;

    /// Human-readable node kind (e.g. `"source"`, `"sql"`, `"sink"`).
    fn node_type(&self) -> &str;

    /// Downcast helper for concrete-type introspection (e.g. extracting
    /// sink-specific details at report time).
    fn as_any(&self) -> &dyn std::any::Any;

    // fn write_output(&self, port_id: &str, df: DataFrame) -> Result<(), DagError>;
    // fn get_input(&self, port_id: &str) -> Result<DataFrame, DagError>;
}

impl Clone for Box<dyn DagNode> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}
