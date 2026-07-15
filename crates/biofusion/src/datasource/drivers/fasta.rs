//! FASTA driver.
//!
//! FASTA has no header; the schema is fully determined by the scanner config,
//! so [`BioDriver::infer_schema`] never needs to look at the bytes.

use std::sync::Arc;

use arrow_schema::SchemaRef;
use datafusion::error::Result;
use oxbow::sequence::FastaScanner;
use oxbow::{CoordSystem, Select};

use super::super::core::{BioBatchIter, BioDriver, BioInput, buf_reader, map_ext};

fn scanner() -> Result<FastaScanner> {
    FastaScanner::new(Select::All, CoordSystem::ZeroHalfOpen).map_err(map_ext)
}

pub struct FastaDriver;

impl BioDriver for FastaDriver {
    const FILE_TYPE: &'static str = "fasta";

    fn infer_schema(_input: &BioInput) -> Result<SchemaRef> {
        let scanner = scanner()?;
        Ok(Arc::new(scanner.schema().clone()))
    }

    fn scan(input: BioInput, batch_size: usize) -> Result<BioBatchIter> {
        let reader = noodles::fasta::io::Reader::new(buf_reader(input.bytes, input.gz));
        let scanner = scanner()?;
        let batches = scanner
            .scan(reader, None, Some(batch_size), None)
            .map_err(map_ext)?;
        Ok(Box::new(batches))
    }
}
