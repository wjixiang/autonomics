use async_trait::async_trait;
use datafusion::prelude::CsvReadOptions;
use datafusion::prelude::DataFrame;
use thiserror::Error;

use crate::data_engine::dag::NodeMeta;
use crate::data_engine::dag::{DagError, DagNode};

/// Errors specific to [`LoadCsvNode`].
///
/// Concrete and structured (carrying the offending path or the underlying
/// arrow/opendal error) instead of a flattened `String` like the previous
/// `DagError::Csv(...)` design. Unified into [`DagError::ExecutionError`]
/// via the `From<LoadCsvError> for DagError` impl below.
#[derive(Debug, Error)]
pub enum LoadCsvError {
    #[error("read csv file '{path}' failed")]
    Read {
        path: String,
        #[source]
        source: fs::opendal::Error,
    },

    #[error("infer csv schema failed")]
    InferSchema(#[source] arrow_schema::ArrowError),

    #[error("build csv reader failed")]
    BuildReader(#[source] arrow_schema::ArrowError),

    #[error("read csv batch failed")]
    ReadBatch(#[source] arrow_schema::ArrowError),
}

impl LoadCsvError {
    /// Stable classification tag for [`DagError::ExecutionError`]. Mirrors the
    /// shape of `csv.<stage>` so callers can switch on stage without
    /// downcasting.
    fn kind(&self) -> &'static str {
        match self {
            Self::Read { .. } => "csv.read",
            Self::InferSchema(_) => "csv.infer_schema",
            Self::BuildReader(_) => "csv.build_reader",
            Self::ReadBatch(_) => "csv.read_batch",
        }
    }
}

impl From<LoadCsvError> for DagError {
    fn from(e: LoadCsvError) -> Self {
        DagError::execution(e.kind(), e)
    }
}

/// Load full dataset into memory as RecordBatchs
pub struct LoadCsvNode {
    meta: NodeMeta,
    csv_file_path: String,
}

impl LoadCsvNode {
    pub fn new(meta: NodeMeta, csv_path: &str) -> Self {
        Self {
            meta,
            csv_file_path: csv_path.to_string(),
        }
    }
}

#[async_trait]
impl DagNode for LoadCsvNode {
    fn meta(&self) -> &NodeMeta {
        &self.meta
    }
    async fn execute(&mut self, _inputs: &[DataFrame]) -> Result<Vec<DataFrame>, DagError> {
        // Wrap explicitly so the error is categorized under `LoadCsvError::Dataset`
        // (kind = "csv.dataset") rather than falling through to `DagError::Dataset`.

        let ctx = self.meta.ctx().clone();
        let df = ctx
            .read_csv(self.csv_file_path.clone(), CsvReadOptions::default())
            .await
            .map_err(|e| DagError::ExecutionError {
                kind: "load_csv_error",
                source: Box::new(e),
            })?;

        Ok(vec![df])
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::data_engine::dag::{DagNode, DagNodeStatus};
    use arrow_array::{Int64Array, StringArray};
    use datafusion::prelude::SessionContext;

    #[tokio::test]
    async fn test_csv_load() {
        let id = "test_id";
        let csv_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_datasets")
            .join("insurance.csv");

        let ctx = Arc::new(SessionContext::new());
        let meta = NodeMeta::new(
            id.to_string(),
            "load_csv_node".to_string(),
            DagNodeStatus::Idle,
            ctx.clone(),
        );
        let mut csv_load_node = LoadCsvNode::new(meta, csv_path.to_str().unwrap());
        let res = csv_load_node.execute(&[]).await.unwrap();

        // Verify node metadata
        assert_eq!(csv_load_node.meta().id(), "test_id");
        assert_eq!(csv_load_node.meta().name(), "load_csv_node");

        assert!(!res.is_empty());

        let df = res.first().unwrap();

        // Verify shape: 1338 rows, 7 columns
        assert_eq!(df.clone().count().await.unwrap(), 1338);
        assert_eq!(df.schema().fields().len(), 7);

        // Verify column names
        let columns: Vec<&str> = df
            .schema()
            .fields()
            .iter()
            .map(|f| f.name().as_str())
            .collect();
        assert_eq!(
            columns,
            vec![
                "age", "sex", "bmi", "children", "smoker", "region", "charges"
            ]
        );

        // Verify first row values
        let batches = df.clone().collect().await.unwrap();
        assert!(!batches.is_empty());

        let batch = &batches[0];
        let age = batch
            .column(0)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap();
        assert_eq!(age.value(0), 19);

        let sex = batch
            .column(1)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(sex.value(0), "female");
    }
}
