pub mod aether;
pub mod config;
pub mod datalake;
pub mod dataset;
pub mod error;
pub mod types;

// Re-exports: core dataset types
pub use dataset::store::{DatasetInfo, DatasetStore, ColumnInfo};
pub use dataset::{AetherDataset, NullPolicy, Provenance};
pub use error::DatasetError;
