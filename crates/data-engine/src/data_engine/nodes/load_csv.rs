use std::io::Cursor;
use std::sync::Arc;

use arrow_csv::{ReaderBuilder, reader::Format};
use arrow_schema::SchemaRef;
use async_trait::async_trait;
use datafusion::common::HashSet;
use fs::OpendalFileStorage;
use thiserror::Error;

use crate::Dataset;
use crate::DatasetStore;
use crate::data_engine::dag::NodeMeta;
use crate::data_engine::dag::{DagError, DagNode, DagNodeStatus};
use crate::dataset::DatasetRef;

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

    #[error(transparent)]
    Dataset(#[from] crate::DatasetError),
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
            Self::Dataset(_) => "csv.dataset",
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
    fs_ref: Arc<OpendalFileStorage>,
    dataset_store_ref: Arc<DatasetStore>,
    csv_file_path: String,
    output_ids: HashSet<String>,
}

impl LoadCsvNode {
    pub fn new(
        id: &str,
        fs_ref: Arc<OpendalFileStorage>,
        dataset_store_ref: Arc<DatasetStore>,
        csv_path: &str,
    ) -> Self {
        Self {
            meta: NodeMeta::new(
                id.to_string(),
                "load_csv_node".to_string(),
                DagNodeStatus::Idle,
            ),
            fs_ref,
            dataset_store_ref,
            csv_file_path: csv_path.to_string(),
            output_ids: HashSet::default(),
        }
    }
}

#[async_trait]
impl DagNode for LoadCsvNode {
    fn meta(&self) -> &NodeMeta {
        &self.meta
    }
    async fn execute(&mut self, _inputs: &[DatasetRef]) -> Result<(), DagError> {
        let op = self.fs_ref.op.clone();
        let path = self.csv_file_path.clone();

        let bytes = op.read(&path).await.map_err(|source| LoadCsvError::Read {
            path: path.clone(),
            source,
        })?;
        let bytes = bytes.to_vec();

        let format = Format::default().with_header(true).with_delimiter(b',');
        let (schema, _) = format
            .infer_schema(Cursor::new(&bytes), Some(1000))
            .map_err(LoadCsvError::InferSchema)?;
        let schema: SchemaRef = Arc::new(schema);

        let reader = ReaderBuilder::new(schema)
            .with_header(true)
            .with_delimiter(b',')
            .build(Cursor::new(&bytes))
            .map_err(LoadCsvError::BuildReader)?;

        let mut batches = Vec::new();
        for batch in reader {
            let batch = batch.map_err(LoadCsvError::ReadBatch)?;
            batches.push(batch);
        }

        // Wrap explicitly so the error is categorized under `LoadCsvError::Dataset`
        // (kind = "csv.dataset") rather than falling through to `DagError::Dataset`.
        let dataset = Dataset::new(batches).map_err(LoadCsvError::from)?;
        let store = self.dataset_store_ref.clone();

        // Store dataset reference id
        self.output_ids.insert(dataset.id().to_string());

        store.put_overwrite(dataset).await;
        Ok(())
    }

    fn get_output_ids(&self) -> Result<&HashSet<String>, DagError> {
        Ok(&self.output_ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_engine::dag::DagNode;

    #[tokio::test]
    async fn test_csv_load() {
        let id = "test_id";
        let csv_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_datasets")
            .join("insurance.csv");
        let csv_bytes = std::fs::read(&csv_path).unwrap();

        let opendal_path = "/test.csv";
        let fs_ref = Arc::new(OpendalFileStorage::new());

        // Write test CSV into in-memory storage
        fs_ref.op.write(opendal_path, csv_bytes).await.unwrap();

        let dataset_store_ref = Arc::new(DatasetStore::new());
        let mut csv_load_node =
            LoadCsvNode::new(id, fs_ref, dataset_store_ref.clone(), opendal_path);
        csv_load_node.execute(&[]).await.unwrap();

        // Verify node metadata
        assert_eq!(csv_load_node.meta().id(), "test_id");
        assert_eq!(csv_load_node.meta().name(), "load_csv_node");

        // Verify dataset was registered in store
        let ids = csv_load_node.get_output_ids().unwrap();
        assert!(!ids.is_empty());

        let dataset_id = ids.iter().next().unwrap();
        let dataset = dataset_store_ref.get(dataset_id).await.unwrap();

        // Verify shape: 1338 rows, 7 columns
        assert_eq!(dataset.row_count(), 1338);
        assert_eq!(dataset.column_count(), 7);

        // Verify column names
        let columns: Vec<&str> = dataset.column_names().collect();
        assert_eq!(
            columns,
            vec![
                "age", "sex", "bmi", "children", "smoker", "region", "charges"
            ]
        );

        // Verify numeric column extraction works
        let ages = dataset
            .extract_f64("age", crate::NullPolicy::default())
            .unwrap();
        assert_eq!(ages.len(), 1338);
        assert_eq!(ages[0], 19.0);

        // Verify string column extraction works
        let sexes = dataset.extract_string("sex").unwrap();
        assert_eq!(sexes[0], Some("female".to_string()));
    }
}
