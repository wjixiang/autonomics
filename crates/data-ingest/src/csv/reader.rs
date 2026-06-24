//! CSV / TSV file ingestor — converts delimited text files to Arrow RecordBatches.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use arrow_array::RecordBatch;
use arrow_csv::{ReaderBuilder, reader::Format};
use arrow_schema::{Schema, SchemaRef};
use async_trait::async_trait;

use crate::error::IngestError;
use crate::trait_def::{FileIngestor, IngestConfig};

/// Ingestor for CSV and TSV files.
///
/// Supports:
/// - `.csv` — comma-separated (delimiter `,`)
/// - `.tsv` — tab-separated (delimiter `\t`)
/// - `.txt` — tab-separated (delimiter `\t`)
///
/// Schema is inferred from the first N rows via `arrow-csv`.
pub struct CsvIngestor;

#[async_trait]
impl FileIngestor for CsvIngestor {
    fn format_name(&self) -> &str {
        "CSV"
    }

    fn file_extensions(&self) -> &[&str] {
        &["csv", "tsv", "txt"]
    }

    async fn infer_schema(&self, path: &Path) -> Result<SchemaRef> {
        let (delimiter, has_header) = detect_format(path);
        let file = std::fs::File::open(path)
            .with_context(|| format!("failed to open '{}'", path.display()))?;

        let format = Format::default()
            .with_header(has_header)
            .with_delimiter(delimiter);

        let (schema, _records_read) = format.infer_schema(file, Some(1000))?;
        Ok(Arc::new(schema))
    }

    async fn ingest(&self, path: &Path, config: IngestConfig) -> Result<Vec<RecordBatch>> {
        let (delimiter, has_header) = detect_format(path);

        // Phase 1: infer schema from the first rows.
        let file = std::fs::File::open(path)
            .with_context(|| format!("failed to open '{}'", path.display()))?;

        let format = Format::default()
            .with_header(has_header)
            .with_delimiter(delimiter);

        let (schema, _records_read) = format
            .infer_schema(file, Some(1000))
            .with_context(|| format!("failed to infer schema from '{}'", path.display()))?;

        // Phase 2: build reader with the inferred schema.
        let file = std::fs::File::open(path)
            .with_context(|| format!("failed to re-open '{}'", path.display()))?;

        let reader = ReaderBuilder::new(Arc::new(schema))
            .with_header(has_header)
            .with_delimiter(delimiter)
            .with_batch_size(config.batch_size)
            .build(file)?;

        let mut batches = Vec::new();
        let mut total_rows = 0usize;

        for batch_result in reader {
            let batch = batch_result
                .with_context(|| format!("failed to read CSV batch from '{}'", path.display()))?;

            let rows = batch.num_rows();

            // Apply limit.
            if let Some(limit) = config.limit {
                if total_rows >= limit {
                    break;
                }
                let remaining = limit - total_rows;
                if rows > remaining {
                    batches.push(batch.slice(0, remaining));
                    break;
                }
            }

            batches.push(batch);
            total_rows += rows;
        }

        if batches.is_empty() {
            return Err(IngestError::EmptyFile {
                path: path.display().to_string(),
            }
            .into());
        }

        Ok(batches)
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Detect delimiter and header presence from the file extension.
fn detect_format(path: &Path) -> (u8, bool) {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    match ext.as_str() {
        "tsv" => (b'\t', true),
        "csv" => (b',', true),
        // .txt — default to tab-separated with header
        _ => (b'\t', true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_format() {
        assert_eq!(
            detect_format(Path::new("data/sample.csv")),
            (b',', true)
        );
        assert_eq!(
            detect_format(Path::new("data/sample.tsv")),
            (b'\t', true)
        );
        assert_eq!(
            detect_format(Path::new("data/sample.txt")),
            (b'\t', true)
        );
    }
}
