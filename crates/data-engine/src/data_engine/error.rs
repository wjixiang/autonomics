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
