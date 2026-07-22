//! BigWig driver.
//!
//! BigWig is a random-access BBI format; the whole object is read into memory
//! and opened via [`bigtools::BigWigRead`] over a [`Cursor`]. Positions are
//! 0-based half-open.

use std::io::Cursor;
use std::sync::Arc;

use arrow_schema::SchemaRef;
use async_trait::async_trait;
use datafusion::error::Result;
use oxbow::bbi::BigWigScanner;
use oxbow::{CoordSystem, Select};

use super::super::core::{BioBatchStream, BioDriver, BioInput, map_ext, sync_stream};

pub struct BigWigDriver;

#[async_trait]
impl BioDriver for BigWigDriver {
    const FILE_TYPE: &'static str = "bigwig";

    async fn infer_schema(input: &BioInput) -> Result<SchemaRef> {
        let bytes = input.fetch_all().await?;
        let reader = bigtools::BigWigRead::open(Cursor::new(bytes)).map_err(map_ext)?;
        let scanner = BigWigScanner::new(
            reader.info().clone(),
            Select::All,
            CoordSystem::ZeroHalfOpen,
        )
        .map_err(map_ext)?;
        Ok(Arc::new(scanner.schema().clone()))
    }

    async fn scan(input: BioInput, batch_size: usize, limit: Option<usize>) -> Result<BioBatchStream> {
        let bytes = input.fetch_all().await?;
        let reader = bigtools::BigWigRead::open(Cursor::new(bytes)).map_err(map_ext)?;
        let scanner = BigWigScanner::new(
            reader.info().clone(),
            Select::All,
            CoordSystem::ZeroHalfOpen,
        )
        .map_err(map_ext)?;
        let batches = scanner
            .scan(reader, None, Some(batch_size), limit)
            .map_err(map_ext)?;
        Ok(sync_stream(batches))
    }
}
