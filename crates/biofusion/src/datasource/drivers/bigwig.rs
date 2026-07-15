//! BigWig driver.
//!
//! BigWig is a random-access BBI format; the whole object is read into memory
//! and opened via [`bigtools::BigWigRead`] over a [`Cursor`]. Positions are
//! 0-based half-open.

use std::io::Cursor;
use std::sync::Arc;

use arrow_schema::SchemaRef;
use datafusion::error::Result;
use oxbow::bbi::BigWigScanner;
use oxbow::{CoordSystem, Select};

use super::super::core::{BioBatchIter, BioDriver, BioInput, map_ext};

pub struct BigWigDriver;

impl BioDriver for BigWigDriver {
    const FILE_TYPE: &'static str = "bigwig";

    fn infer_schema(input: &BioInput) -> Result<SchemaRef> {
        let reader =
            bigtools::BigWigRead::open(Cursor::new(input.bytes.clone())).map_err(map_ext)?;
        let scanner = BigWigScanner::new(
            reader.info().clone(),
            Select::All,
            CoordSystem::ZeroHalfOpen,
        )
        .map_err(map_ext)?;
        Ok(Arc::new(scanner.schema().clone()))
    }

    fn scan(input: BioInput, batch_size: usize) -> Result<BioBatchIter> {
        let reader = bigtools::BigWigRead::open(Cursor::new(input.bytes)).map_err(map_ext)?;
        let scanner = BigWigScanner::new(
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
