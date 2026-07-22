//! FASTQ driver. Header-less; schema is determined by the scanner config.

use std::sync::Arc;

use arrow_schema::SchemaRef;
use async_trait::async_trait;
use datafusion::error::Result;
use oxbow::Select;
use oxbow::sequence::FastqScanner;

use super::super::core::{BioBatchStream, BioDriver, BioInput, buf_reader, map_ext, sync_stream};

fn scanner() -> Result<FastqScanner> {
    FastqScanner::new(Select::All).map_err(map_ext)
}

pub struct FastqDriver;

#[async_trait]
impl BioDriver for FastqDriver {
    const FILE_TYPE: &'static str = "fastq";

    async fn infer_schema(_input: &BioInput) -> Result<SchemaRef> {
        let scanner = scanner()?;
        Ok(Arc::new(scanner.schema().clone()))
    }

    async fn scan(input: BioInput, batch_size: usize, limit: Option<usize>) -> Result<BioBatchStream> {
        let bytes = input.fetch_all().await?;
        let reader = noodles::fastq::io::Reader::new(buf_reader(bytes, input.gz));
        let scanner = scanner()?;
        let batches = scanner
            .scan(reader, None, Some(batch_size), limit)
            .map_err(map_ext)?;
        Ok(sync_stream(batches))
    }
}
