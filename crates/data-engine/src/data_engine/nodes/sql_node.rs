use async_trait::async_trait;
use datafusion::common::HashSet;

use crate::data_engine::dag::{DagError, DagNode, DagNodeStatus, NodeMeta};
use crate::dataset::DatasetRef;

pub struct SqlNode {
    meta: NodeMeta,
    output_ids: HashSet<String>,
}

impl SqlNode {
    pub fn new(id: String, name: String) -> Self {
        Self {
            meta: NodeMeta::new(id, name, DagNodeStatus::Idle),
            output_ids: HashSet::new(),
        }
    }
}

#[async_trait]
impl DagNode for SqlNode {
    fn meta(&self) -> &NodeMeta {
        &self.meta
    }

    async fn execute(&mut self, _inputs: &[DatasetRef]) -> Result<(), DagError> {
        todo!()
    }

    fn get_output_ids(&self) -> Result<&HashSet<String>, DagError> {
        todo!()
    }
}
