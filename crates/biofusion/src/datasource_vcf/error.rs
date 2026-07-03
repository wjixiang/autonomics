use derive_more::From;
use oxbow::OxbowError;
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, From)]
pub enum Error {
    #[from]
    Custom(String),

    #[from]
    Datafusion(datafusion::error::DataFusionError),

    #[from]
    DatafusionObjectStorePath(datafusion::object_store::path::Error),

    #[from]
    DatafusionObjectStore(datafusion::object_store::Error),

    #[from]
    Io(std::io::Error),

    #[from]
    Oxbow(OxbowError),
}
