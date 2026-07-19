use std::fmt::Display;

use async_trait::async_trait;
use datafusion::common::HashMap;
use schemars::{schema_for, JsonSchema};
use serde::Deserialize;

use crate::{
    dag::{DagError, DagNode, NodeInput, NodePorts, graph::PortOutputs},
    dataset::{BuiltinDataset, get_builtin_dataset},
    node_registry::registry::{NodeCtx, NodeFactory},
};

#[derive(Clone)]
pub struct MockNode {
    meta: NodePorts,
}

#[derive(Debug)]
pub enum MockNodeError {
    MockError { msg: String },
}

impl Display for MockNodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MockNodeError::MockError { msg } => write!(f, "{msg}"),
        }
    }
}

impl From<&str> for MockNodeError {
    fn from(value: &str) -> Self {
        MockNodeError::MockError {
            msg: value.to_string(),
        }
    }
}

#[derive(Debug, JsonSchema, Deserialize)]
pub struct MockNodeSpec {}

pub struct MockNodeFactory {}

/// Static port layout for every [`MockNode`]: no declared ports (the mock
/// emits the iris dataset on the default output at execution time without a
/// statically-declared port).
fn port_layout() -> NodePorts {
    NodePorts::new()
}

impl NodeFactory for MockNodeFactory {
    fn kind(&self) -> &'static str {
        "mock"
    }

    fn spec_schema(&self) -> schemars::Schema {
        schema_for!(MockNodeSpec)
    }

    fn ports(&self) -> NodePorts {
        port_layout()
    }

    fn build(
        &self,
        spec: serde_json::Value,
        _node_ctx: NodeCtx,
    ) -> crate::node_registry::error::Result<Box<dyn DagNode>> {
        let _node_spec: MockNodeSpec = serde_json::from_value(spec)?;
        Ok(Box::new(MockNode::default()))
    }
}

impl MockNode {}
impl Default for MockNode {
    fn default() -> Self {
        // A source-style mock: no inputs, one output port "iris".
        Self {
            meta: port_layout(),
        }
    }
}
#[async_trait]
impl DagNode for MockNode {
    fn ports(&self) -> &NodePorts {
        &self.meta
    }

    fn clone_box(&self) -> Box<dyn DagNode> {
        Box::new((*self).clone())
    }

    fn kind(&self) -> &'static str {
        "mock"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    /// Input data injected by the scheduler when the node runs.
    async fn execute(&mut self, _inputs: &[NodeInput]) -> Result<PortOutputs, DagError> {
        let test_dataset = get_builtin_dataset(BuiltinDataset::Iris).await;
        let mut res: PortOutputs = HashMap::new();
        res.insert(0, test_dataset);
        Ok(res)
    }
}
