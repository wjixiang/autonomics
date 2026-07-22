//! BigBed driver.
//!
//! BigBed is a random-access BBI format; the whole object is read into memory
//! and opened via [`bigtools::BigBedRead`] over a [`Cursor`]. Positions are
//! 0-based half-open.

use std::io::Cursor;
use std::sync::Arc;

use arrow_schema::SchemaRef;
use async_trait::async_trait;
use datafusion::error::Result;
use oxbow::bbi::{BedSchema, BigBedScanner};
use oxbow::{CoordSystem, Select};

use super::super::core::{BioBatchStream, BioDriver, BioInput, map_ext, sync_stream};

fn bed_schema() -> Result<BedSchema> {
    "bed3".parse::<BedSchema>().map_err(map_ext)
}

pub struct BigBedDriver;

#[async_trait]
impl BioDriver for BigBedDriver {
    const FILE_TYPE: &'static str = "bigbed";

    async fn infer_schema(input: &BioInput) -> Result<SchemaRef> {
        let bytes = input.fetch_all().await?;
        let reader = bigtools::BigBedRead::open(Cursor::new(bytes)).map_err(map_ext)?;
        let scanner = BigBedScanner::new(
            bed_schema()?,
            reader.info().clone(),
            Select::All,
            CoordSystem::ZeroHalfOpen,
        )
        .map_err(map_ext)?;
        Ok(Arc::new(scanner.schema().clone()))
    }

    async fn scan(input: BioInput, batch_size: usize, limit: Option<usize>) -> Result<BioBatchStream> {
        let bytes = input.fetch_all().await?;
        let reader = bigtools::BigBedRead::open(Cursor::new(bytes)).map_err(map_ext)?;
        let scanner = BigBedScanner::new(
            bed_schema()?,
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
