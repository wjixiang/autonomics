//! `data-ingest` — Multi-format file ingestion for the datalake.
//!
//! Converts structured files (VCF, CSV, …) into Arrow [`RecordBatch`]es
//! that can be loaded into [`AetherDataset`] and written to Iceberg.
//!
//! # Quick start
//!
//! ```ignore
//! use data_ingest::ingest_to_dataset;
//!
//! let dataset = ingest_to_dataset(
//!     std::path::Path::new("data/sample.vcf.gz"),
//!     "my_variants",
//!     Default::default(),
//! ).await?;
//! ```
//!
//! # Architecture
//!
//! ```text
//! File → FileIngestor → Vec<RecordBatch> → AetherDataset → Iceberg
//!              │
//!              ├── VcfIngestor  (via vcf-arrow)
//!              ├── CsvIngestor  (via arrow-csv)
//!              └── (future formats)
//! ```

pub mod csv;
pub mod error;
pub mod registry;
pub mod tools;
pub mod trait_def;
pub mod vcf;

use std::path::Path;

use anyhow::Result;

use datalake::{AetherDataset, Provenance};

// Public re-exports
pub use csv::CsvIngestor;
pub use error::IngestError;
pub use registry::IngestRegistry;
pub use trait_def::{FileIngestor, IngestConfig};
pub use vcf::VcfIngestor;

/// Ingest a file and produce an [`AetherDataset`] ready for use.
///
/// This is the primary integration point with the datalake crate.
/// The registry automatically picks the correct ingestor based on the
/// file extension.
///
/// # Errors
///
/// Returns an error if:
/// - The file format is not supported
/// - The file cannot be parsed
/// - No records are found
/// - The resulting `RecordBatch` cannot be assembled
pub async fn ingest_to_dataset(
    path: &Path,
    name: impl Into<String>,
    config: IngestConfig,
) -> Result<AetherDataset> {
    let name = name.into();
    let registry = IngestRegistry::with_defaults();
    let batches = registry.ingest_file(path, config).await?;

    let format_name = registry
        .find_ingestor(path)
        .map(|i| i.format_name().to_owned())
        .unwrap_or_else(|_| "unknown".into());

    let dataset = AetherDataset::new(&name, batches)?;
    Ok(dataset.with_provenance(Provenance::FileIngest {
        path: path.display().to_string(),
        format: format_name,
    }))
}
