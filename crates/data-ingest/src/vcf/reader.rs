//! VCF file ingestor — converts VCF/VCF.gz to Arrow RecordBatches.

use std::path::Path;

use anyhow::{Context, Result};
use arrow_array::RecordBatch;
use arrow_schema::{DataType, SchemaRef};

use vcf_arrow::vcf::{VcfParseResult, VcfReader};

use crate::error::IngestError;
use crate::trait_def::{FileIngestor, IngestConfig};
use super::schema::vcf_arrow_schema;

/// Ingestor for VCF and VCF.gz files.
pub struct VcfIngestor;

#[async_trait::async_trait]
impl FileIngestor for VcfIngestor {
    fn format_name(&self) -> &str {
        "VCF"
    }

    fn file_extensions(&self) -> &[&str] {
        &["vcf", "vcf.gz", "bgz"]
    }

    async fn infer_schema(&self, path: &Path) -> Result<SchemaRef> {
        let result = parse_vcf(path)
            .map_err(|e| IngestError::vcf_with_path(path, e.to_string()))?;
        Ok(result.schema)
    }

    async fn ingest(&self, path: &Path, config: IngestConfig) -> Result<Vec<RecordBatch>> {
        let parsed = parse_vcf(path)
            .map_err(|e| IngestError::vcf_with_path(path, e.to_string()))?;

        if parsed.batch.num_rows() == 0 {
            return Err(IngestError::EmptyFile {
                path: path.display().to_string(),
            }
            .into());
        }

        let limit = config.limit.unwrap_or(parsed.batch.num_rows());
        let total_rows = limit.min(parsed.batch.num_rows());

        // If the entire batch fits within the limit, return it as-is.
        if total_rows <= config.batch_size {
            let batch = if total_rows < parsed.batch.num_rows() {
                parsed.batch.slice(0, total_rows)
            } else {
                parsed.batch
            };
            return Ok(vec![batch]);
        }

        // Slice the single RecordBatch into smaller partitions.
        let mut batches = Vec::new();
        let mut offset = 0usize;
        while offset < total_rows {
            let take = config.batch_size.min(total_rows - offset);
            batches.push(parsed.batch.slice(offset, take));
            offset += take;
        }

        Ok(batches)
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Intermediate struct holding both the RecordBatch and its schema.
struct ParsedVcf {
    batch: RecordBatch,
    schema: SchemaRef,
}

/// Parse a VCF or VCF.gz file into a single `RecordBatch`.
fn parse_vcf(path: &Path) -> Result<ParsedVcf, IngestError> {
    let path_str = path.to_str().ok_or_else(|| IngestError::Io {
        path: path.display().to_string(),
        message: "path is not valid UTF-8".into(),
    })?;

    let reader = if path_str.ends_with(".gz") || path_str.ends_with(".bgz") {
        VcfReader::convert_from_gz(path_str)?
    } else {
        let content = std::fs::read_to_string(path)?;
        VcfReader::convert_from_str(&content)?
    };

    let result = reader.parse_into_arrow()?;

    // Collect sample column metadata for schema construction.
    let sample_keys: Vec<String> = result.samples.keys().cloned().collect();
    let mut sample_fields: Vec<(String, DataType)> = Vec::new();
    for (key, arr) in &result.samples {
        sample_fields.push((key.clone(), arr.data_type().clone()));
    }

    let schema = vcf_arrow_schema(&sample_keys, &sample_fields);

    // Assemble columns: 8 fixed + dynamic sample columns.
    let mut columns: Vec<std::sync::Arc<dyn arrow_array::Array>> = vec![
        result.chrom.clone(),
        result.pos.clone(),
        result.id.clone(),
        result._ref.clone(),
        result.alt.clone(),
        result.qual.clone(),
        result.filter.clone(),
        result.info.clone(),
    ];

    // Append sample columns in the order they appear in the HashMap
    // (deterministic for small maps, and matching the schema construction).
    for key in &sample_keys {
        if let Some(arr) = result.samples.get(key) {
            columns.push(arr.clone());
        }
    }

    let batch = RecordBatch::try_new(schema.clone(), columns)?;

    Ok(ParsedVcf { batch, schema })
}
