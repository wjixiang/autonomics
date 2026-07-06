use std::fmt::Display;

use async_trait::async_trait;
use datafusion::prelude::DataFrame;

use crate::data_engine::dag::{DagError, DagNode, NodeInput, NodeMeta, graph::NamedDataFrames};

#[derive(Clone)]
pub struct PreviewNode {
    meta: NodeMeta,
}

#[derive(Debug)]
pub enum PreviewNodeError {
    PreviewError { msg: String },
}

impl Display for PreviewNodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PreviewNodeError::PreviewError { msg } => write!(f, "{msg}"),
        }
    }
}

impl From<&str> for PreviewNodeError {
    fn from(value: &str) -> Self {
        PreviewNodeError::PreviewError {
            msg: value.to_string(),
        }
    }
}

impl PreviewNode {}
impl Default for PreviewNode {
    fn default() -> Self {
        let meta = NodeMeta::new("preview_node");
        Self { meta }
    }
}
#[async_trait]
impl DagNode for PreviewNode {
    fn meta(&self) -> &NodeMeta {
        todo!()
    }

    fn clone_box(&self) -> Box<dyn DagNode> {
        Box::new((*self).clone())
    }

    /// Input data injected by the scheduler when the node runs.
    async fn execute(&mut self, inputs: &[NodeInput]) -> Result<NamedDataFrames, DagError> {
        todo!()
    }
}
