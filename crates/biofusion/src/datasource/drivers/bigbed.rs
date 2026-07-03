//! BigBed driver.
//!
//! BigBed is a random-access BBI format; the whole object is read into memory
//! and opened via [`bigtools::BigBedRead`] over a [`Cursor`]. Positions are
//! 0-based half-open.

use std::io::Cursor;
use std::sync::Arc;

use arrow_schema::SchemaRef;
use datafusion::error::Result;
use oxbow::bbi::{BigBedScanner, BedSchema};
use oxbow::{CoordSystem, Select};

use super::super::core::{map_ext, BioBatchIter, BioDriver, BioInput};

fn bed_schema() -> Result<BedSchema> {
    "bed3".parse::<BedSchema>().map_err(map_ext)
}

pub struct BigBedDriver;

impl BioDriver for BigBedDriver {
    const FILE_TYPE: &'static str = "bigbed";

    fn infer_schema(input: &BioInput) -> Result<SchemaRef> {
        let reader = bigtools::BigBedRead::open(Cursor::new(input.bytes.clone()))
            .map_err(map_ext)?;
        let scanner = BigBedScanner::new(
            bed_schema()?,
            reader.info().clone(),
            Select::All,
            CoordSystem::ZeroHalfOpen,
        )
        .map_err(map_ext)?;
        Ok(Arc::new(scanner.schema().clone()))
    }

    fn scan(input: BioInput, batch_size: usize) -> Result<BioBatchIter> {
        let reader = bigtools::BigBedRead::open(Cursor::new(input.bytes)).map_err(map_ext)?;
        let scanner = BigBedScanner::new(
            bed_schema()?,
            reader.info().clone(),
            Select::All,
            CoordSystem::ZeroHalfOpen,
        )
        .map_err(map_ext)?;
        let batches = scanner
            .scan(reader, None, Some(batch_size), None)
            .map_err(map_ext)?;
        Ok(Box::new(batches))
    }
}
