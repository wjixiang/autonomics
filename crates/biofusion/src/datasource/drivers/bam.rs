//! BAM driver.
//!
//! BAM is BGZF-compressed; [`noodles::bam::io::Reader::new`] wraps the inner
//! reader in a BGZF decoder itself, so the `gz` flag is ignored. Positions are
//! 1-based.

use std::sync::Arc;

use arrow_schema::SchemaRef;
use async_trait::async_trait;
use datafusion::error::Result;
use oxbow::alignment::BamScanner;
use oxbow::{CoordSystem, Select};

use super::super::core::{BioBatchStream, BioDriver, BioInput, byte_reader, map_ext, sync_stream};

fn scanner(header: noodles::sam::Header) -> Result<BamScanner> {
    BamScanner::new(header, Select::All, None, CoordSystem::OneClosed).map_err(map_ext)
}

pub struct BamDriver;

#[async_trait]
impl BioDriver for BamDriver {
    const FILE_TYPE: &'static str = "bam";

    async fn infer_schema(input: &BioInput) -> Result<SchemaRef> {
        let bytes = input.fetch_all().await?;
        let mut reader = noodles::bam::io::Reader::new(byte_reader(bytes));
        let header = reader.read_header().map_err(map_ext)?;
        let scanner = scanner(header)?;
        Ok(Arc::new(scanner.schema().clone()))
    }

    async fn scan(
        input: BioInput,
        batch_size: usize,
        limit: Option<usize>,
    ) -> Result<BioBatchStream> {
        let bytes = input.fetch_all().await?;
        let mut reader = noodles::bam::io::Reader::new(byte_reader(bytes));
        let header = reader.read_header().map_err(map_ext)?;
        let scanner = scanner(header)?;
        let batches = scanner
            .scan(reader, None, Some(batch_size), limit)
            .map_err(map_ext)?;
        Ok(sync_stream(batches))
    }
}
