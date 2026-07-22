//! GFF driver. Mirrors [`super::gtf`]; 8 standard columns, 1-based positions.

use std::sync::Arc;

use arrow_schema::SchemaRef;
use async_trait::async_trait;
use datafusion::error::Result;
use oxbow::gxf::GffScanner;
use oxbow::{CoordSystem, Select};

use super::super::core::{BioBatchStream, BioDriver, BioInput, buf_reader, map_ext, sync_stream};

fn scanner() -> Result<GffScanner> {
    GffScanner::new(None, Select::All, None, CoordSystem::OneClosed).map_err(map_ext)
}

pub struct GffDriver;

#[async_trait]
impl BioDriver for GffDriver {
    const FILE_TYPE: &'static str = "gff";

    async fn infer_schema(_input: &BioInput) -> Result<SchemaRef> {
        let scanner = scanner()?;
        Ok(Arc::new(scanner.schema().clone()))
    }

    async fn scan(input: BioInput, batch_size: usize, limit: Option<usize>) -> Result<BioBatchStream> {
        let bytes = input.fetch_all().await?;
        let reader = noodles::gff::io::Reader::new(buf_reader(bytes, input.gz));
        let scanner = scanner()?;
        let batches = scanner
            .scan(reader, None, Some(batch_size), limit)
            .map_err(map_ext)?;
        Ok(sync_stream(batches))
    }
}
