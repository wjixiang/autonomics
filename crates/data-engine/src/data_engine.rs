use std::sync::Arc;

use fs::OpendalFileStorage;

use crate::{DataSession, DatasetStore, data_engine::dag::DAG};
pub mod dag;
pub mod nodes;

/// `DataEngine` is the core object that implements the data analysis engine.
/// It orchestrates ingestion, transformation, and querying of datasets
/// across the datalake.
pub struct DataEngine {
    // Data layer
    /// In-memory data storage
    dataset_store: Arc<DatasetStore>,
    iceberg_session: Arc<DataSession>,
    file_session: Arc<OpendalFileStorage>,

    dag: DAG,
}

impl DataEngine {
    pub fn add_load_node(&mut self) {}
}
