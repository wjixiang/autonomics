use std::fmt::Display;

use async_trait::async_trait;
use datafusion::prelude::DataFrame;

use crate::{
    data_engine::dag::{DagError, DagNode, NodeInput, NodeMeta},
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
        let meta = NodeMeta::new("test_node");
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

    /// Input data injected by the scheduler when the node runs.
    async fn execute(&mut self, inputs: &[NodeInput]) -> Result<Vec<DataFrame>, DagError> {
        let test_dataset = get_builtin_dataset(BuiltinDataset::Iris).await;
        Ok(vec![test_dataset])
    }
}
