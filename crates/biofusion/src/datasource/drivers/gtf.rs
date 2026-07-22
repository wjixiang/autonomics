//! GTF driver.
//!
//! Header-less. Only the 8 standard GTF columns are produced (`attr_defs =
//! None` → no attributes struct column); positions are 1-based.

use std::sync::Arc;

use arrow_schema::SchemaRef;
use async_trait::async_trait;
use datafusion::error::Result;
use oxbow::gxf::GtfScanner;
use oxbow::{CoordSystem, Select};

use super::super::core::{BioBatchStream, BioDriver, BioInput, buf_reader, map_ext, sync_stream};

fn scanner() -> Result<GtfScanner> {
    GtfScanner::new(None, Select::All, None, CoordSystem::OneClosed).map_err(map_ext)
}

pub struct GtfDriver;

#[async_trait]
impl BioDriver for GtfDriver {
    const FILE_TYPE: &'static str = "gtf";

    async fn infer_schema(_input: &BioInput) -> Result<SchemaRef> {
        let scanner = scanner()?;
        Ok(Arc::new(scanner.schema().clone()))
    }

    async fn scan(input: BioInput, batch_size: usize, limit: Option<usize>) -> Result<BioBatchStream> {
        let bytes = input.fetch_all().await?;
        let reader = noodles::gtf::io::Reader::new(buf_reader(bytes, input.gz));
        let scanner = scanner()?;
        let batches = scanner
            .scan(reader, None, Some(batch_size), limit)
            .map_err(map_ext)?;
        Ok(sync_stream(batches))
    }
}
