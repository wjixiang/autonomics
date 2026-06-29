use arrow_array::RecordBatch;
use datafusion::common::HashMap;
use thiserror::Error;

type NodeId = String;
pub struct DAG {
    nodes: HashMap<NodeId, Box<dyn DagNode>>,
    edges: Vec<(NodeId, NodeId)>,
}

pub struct DagScheduler {}

#[derive(Debug, Error)]
pub enum DagExecutionError {}

trait DataPayload {
    type Record;
    fn collect(self) -> Vec<RecordBatch>;
}

trait DagNode {
    fn name(&self) -> &str;
    fn execute(&self, input: Vec<RecordBatch>) -> Result<Vec<RecordBatch>, DagExecutionError>;
}

/// Load full dataset into memory as RecordBatchs
pub struct LoadDagNode {}
