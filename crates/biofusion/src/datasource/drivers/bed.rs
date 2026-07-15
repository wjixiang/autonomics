//! BED driver.
//!
//! Header-less. The parsing interpretation is fixed to a 3-column BED schema
//! (`bed3`); positions are emitted in the 0-based half-open coordinate system.

use std::sync::Arc;

use arrow_schema::SchemaRef;
use datafusion::error::Result;
use oxbow::bed::{BedScanner, BedSchema};
use oxbow::{CoordSystem, Select};

use super::super::core::{BioBatchIter, BioDriver, BioInput, buf_reader, map_ext};

fn bed_schema() -> Result<BedSchema> {
    "bed3".parse::<BedSchema>().map_err(map_ext)
}

fn scanner() -> Result<BedScanner> {
    BedScanner::new(bed_schema()?, Select::All, CoordSystem::ZeroHalfOpen).map_err(map_ext)
}

pub struct BedDriver;

impl BioDriver for BedDriver {
    const FILE_TYPE: &'static str = "bed";

    fn infer_schema(_input: &BioInput) -> Result<SchemaRef> {
        let scanner = scanner()?;
        Ok(Arc::new(scanner.schema().clone()))
    }

    fn scan(input: BioInput, batch_size: usize) -> Result<BioBatchIter> {
        let reader = noodles::bed::io::Reader::<3, _>::new(buf_reader(input.bytes, input.gz));
        let scanner = scanner()?;
        let batches = scanner
            .scan(reader, None, Some(batch_size), None)
            .map_err(map_ext)?;
        Ok(Box::new(batches))
    }
}
