//! SAM driver.
//!
//! The SAM header (embedded at the top of the file) drives both schema
//! inference and the scan. Positions are 1-based.

use std::sync::Arc;

use arrow_schema::SchemaRef;
use async_trait::async_trait;
use datafusion::error::Result;
use oxbow::alignment::SamScanner;
use oxbow::{CoordSystem, Select};

use super::super::core::{BioBatchStream, BioDriver, BioInput, buf_reader, map_ext, sync_stream};

fn scanner(header: noodles::sam::Header) -> Result<SamScanner> {
    SamScanner::new(header, Select::All, None, CoordSystem::OneClosed).map_err(map_ext)
}

pub struct SamDriver;

#[async_trait]
impl BioDriver for SamDriver {
    const FILE_TYPE: &'static str = "sam";

    async fn infer_schema(input: &BioInput) -> Result<SchemaRef> {
        let bytes = input.fetch_all().await?;
        let mut reader = noodles::sam::io::Reader::new(buf_reader(bytes, input.gz));
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
        let mut reader = noodles::sam::io::Reader::new(buf_reader(bytes, input.gz));
        let header = reader.read_header().map_err(map_ext)?;
        let scanner = scanner(header)?;
        let batches = scanner
            .scan(reader, None, Some(batch_size), limit)
            .map_err(map_ext)?;
        Ok(sync_stream(batches))
    }
}
