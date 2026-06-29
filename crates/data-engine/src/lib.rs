pub mod data_session;
pub mod config;
pub mod data_engine;
pub mod datalake;
pub mod dataset;
pub mod error;
pub mod types;

// Re-exports: core dataset types
pub use data_session::DataSession;
pub use dataset::store::{ColumnInfo, DatasetInfo, DatasetStore};
pub use dataset::{AetherDataset, NullPolicy, Provenance};
pub use error::DatasetError;
