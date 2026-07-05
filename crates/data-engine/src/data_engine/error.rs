pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Custom(String),

    #[error("Datalake error")]
    DatalakeError(#[from] datalake::error::Error),

    #[error(transparent)]
    Dag(#[from] crate::data_engine::dag::DagError),
}

#[deprecated]
#[derive(thiserror::Error, Debug)]
pub enum DatasetError {
    #[error("dataset '{name}' not found")]
    NotFound { name: String },

    #[error("column '{column}' not found in dataset '{dataset}'")]
    ColumnNotFound { column: String, dataset: String },

    #[error("column '{column}' has non-numeric type {actual}")]
    NotNumeric { column: String, actual: String },

    #[error("SQL execution failed: {message}")]
    SqlError { message: String },

    #[error("empty dataset '{name}'")]
    EmptyDataset { name: String },

    #[error("error occured during building datafusion context")]
    BuildCtxFaild { message: String },

    #[error("cannot build dataset: {message}")]
    Build { message: String },

    #[error("null value encountered in column '{column}'")]
    HasNulls { column: String },

    #[error(transparent)]
    Arrow(#[from] arrow::error::ArrowError),

    #[error(transparent)]
    DataFusion(#[from] datafusion::error::DataFusionError),
}
