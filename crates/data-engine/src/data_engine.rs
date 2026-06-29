use crate::DatasetStore;
pub mod dag;

/// `DataEngine` is the core object that implements the data analysis engine.
/// It orchestrates ingestion, transformation, and querying of datasets
/// across the datalake.
pub struct DataEngine {
    // Data layer
    /// In-memory data storage
    dataset_store: DatasetStore,
}
