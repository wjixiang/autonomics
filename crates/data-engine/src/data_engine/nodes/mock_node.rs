use std::fmt::Display;

use async_trait::async_trait;
use datafusion::common::HashMap;

use crate::{
    data_engine::dag::{DagError, DagNode, NodeInput, NodeMeta, graph::PortOutputs},
    dataset::{BuiltinDataset, get_builtin_dataset},
};

#[derive(Clone)]
pub struct MockNode {
    meta: NodeMeta,
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

impl MockNode {}
impl Default for MockNode {
    fn default() -> Self {
        // A source-style mock: no inputs, one output port "iris".
        let meta = NodeMeta::new();
        Self { meta }
    }
}
#[async_trait]
impl DagNode for MockNode {
    fn meta(&self) -> &NodeMeta {
        &self.meta
    }

    fn clone_box(&self) -> Box<dyn DagNode> {
        Box::new((*self).clone())
    }

    fn node_type(&self) -> &str {
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
