//! Core trait for file format ingestors.

use std::path::Path;

use anyhow::Result;
use arrow_array::RecordBatch;
use arrow_schema::SchemaRef;
use async_trait::async_trait;

/// Configuration for an ingestion run.
#[derive(Debug, Clone)]
pub struct IngestConfig {
    /// Target number of rows per `RecordBatch` partition.
    pub batch_size: usize,
    /// Maximum number of rows to ingest (`None` = all rows).
    pub limit: Option<usize>,
}

impl Default for IngestConfig {
    fn default() -> Self {
        Self {
            batch_size: 8192,
            limit: None,
        }
    }
}

/// A format-specific ingestor that converts files to Arrow `RecordBatch`es.
///
/// Implementations handle parsing, validation, and batching for a single
/// file format (VCF, CSV, etc.). The trait is async to support future
/// reads from remote / object storage.
#[async_trait]
pub trait FileIngestor: Send + Sync {
    /// Human-readable format name (e.g. `"VCF"`, `"CSV"`).
    fn format_name(&self) -> &str;

    /// File extensions this ingestor handles (e.g. `["vcf", "vcf.gz"]`).
    fn file_extensions(&self) -> &[&str];

    /// Peek at the file and return the Arrow schema that will be produced.
    ///
    /// Must be cheap — ideally reads the header only, not the whole file.
    async fn infer_schema(&self, path: &Path) -> Result<SchemaRef>;

    /// Parse the file and return a vector of `RecordBatch` partitions.
    ///
    /// Each yielded batch has up to `config.batch_size` rows. If the total
    /// row count exceeds `config.limit`, only the first `limit` rows are
    /// returned.
    async fn ingest(&self, path: &Path, config: IngestConfig) -> Result<Vec<RecordBatch>>;
}
