//! `datalake` — Iceberg connection management.
//!
//! Owns the REST catalog lifecycle, the DataFusion `IcebergCatalogProvider`,
//! and the high-level namespace/table operations. Layering rule: this crate
//! must not depend on `data-engine` or any higher-level orchestration; it
//! only talks to Iceberg + DataFusion + opendal storage.

pub mod config;
pub mod datalake;
pub mod error;

pub use config::IcebergConfig;
pub use datalake::Datalake;
