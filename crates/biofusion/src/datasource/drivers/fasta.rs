//! FASTA driver.
//!
//! FASTA has no header; the schema is fully determined by the scanner config,
//! so [`BioDriver::infer_schema`] never needs to look at the bytes.

use std::sync::Arc;

use arrow_schema::SchemaRef;
use async_trait::async_trait;
use datafusion::error::Result;
use oxbow::sequence::FastaScanner;
use oxbow::{CoordSystem, Select};

use super::super::core::{BioBatchStream, BioDriver, BioInput, buf_reader, map_ext, sync_stream};

fn scanner() -> Result<FastaScanner> {
    FastaScanner::new(Select::All, CoordSystem::ZeroHalfOpen).map_err(map_ext)
}

pub struct FastaDriver;

#[async_trait]
impl BioDriver for FastaDriver {
    const FILE_TYPE: &'static str = "fasta";

    async fn infer_schema(_input: &BioInput) -> Result<SchemaRef> {
        let scanner = scanner()?;
        Ok(Arc::new(scanner.schema().clone()))
    }

    async fn scan(
        input: BioInput,
        batch_size: usize,
        limit: Option<usize>,
    ) -> Result<BioBatchStream> {
        let bytes = input.fetch_all().await?;
        let reader = noodles::fasta::io::Reader::new(buf_reader(bytes, input.gz));
        let scanner = scanner()?;
        let batches = scanner
            .scan(reader, None, Some(batch_size), limit)
            .map_err(map_ext)?;
        Ok(sync_stream(batches))
    }
}
