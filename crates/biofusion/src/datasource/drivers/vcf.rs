//! VCF driver.

use std::sync::Arc;

use arrow_schema::SchemaRef;
use datafusion::error::Result;
use oxbow::variant::VcfScanner;
use oxbow::{CoordSystem, Select};

use super::super::core::{BioBatchIter, BioDriver, BioInput, buf_reader, map_ext};

/// Build an oxbow [`VcfScanner`] selecting every contig / field / sample
/// (full schema). Shared by schema inference and the runtime scan so they
/// agree on the produced schema.
fn scanner(header: noodles::vcf::Header) -> Result<VcfScanner> {
    VcfScanner::new(
        header,
        Select::All,
        Select::All,
        Select::All,
        None,
        Select::All,
        None,
        CoordSystem::OneClosed,
    )
    .map_err(map_ext)
}

pub struct VcfDriver;

impl BioDriver for VcfDriver {
    const FILE_TYPE: &'static str = "vcf";

    fn infer_schema(input: &BioInput) -> Result<SchemaRef> {
        let mut reader = noodles::vcf::io::Reader::new(buf_reader(input.bytes.clone(), input.gz));
        let header = reader.read_header().map_err(map_ext)?;
        let scanner = scanner(header)?;
        Ok(Arc::new(scanner.schema().clone()))
    }

    fn scan(input: BioInput, batch_size: usize) -> Result<BioBatchIter> {
        let mut reader = noodles::vcf::io::Reader::new(buf_reader(input.bytes, input.gz));
        let header = reader.read_header().map_err(map_ext)?;
        let scanner = scanner(header)?;
        let batches = scanner
            .scan(reader, None, Some(batch_size), None)
            .map_err(map_ext)?;
        Ok(Box::new(batches))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    /// Parse the fixture VCF.gz file end-to-end and verify row/batch counts.
    ///
    /// Run with: cargo test -p biofusion -- vcf::tests::parse_vcf_gz --nocapture
    #[test]
    fn parse_vcf_gz() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/sample.vcf.gz");
        let raw = fs::read(&path).expect("failed to read VCF.gz");

        println!("file: {}", path.display());
        println!("compressed size: {} bytes", raw.len());

        let bytes = bytes::Bytes::from(raw);
        let input = BioInput {
            gz: true,
            bytes: bytes.clone(),
        };

        // --- schema inference ---
        let schema = VcfDriver::infer_schema(&input).expect("infer_schema failed");
        println!("schema: {} columns", schema.fields().len());
        assert!(schema.fields().len() > 0, "schema should have columns");

        // --- full scan ---
        let batch_size = 8192;
        let batches = VcfDriver::scan(input, batch_size).expect("scan failed");
        let mut total_rows = 0usize;
        let mut total_batches = 0usize;
        for batch in batches {
            let batch = batch.expect("batch error");
            total_rows += batch.num_rows();
            total_batches += 1;
        }

        println!("scan: {total_batches} batches, {total_rows} rows");
        assert!(total_rows > 0, "VCF.gz should contain rows");
        assert!(total_batches > 0, "should produce at least one batch");
    }
}
