//! BAM driver.
//!
//! BAM is BGZF-compressed; [`noodles::bam::io::Reader::new`] wraps the inner
//! reader in a BGZF decoder itself, so the `gz` flag is ignored. Positions are
//! 1-based.

use std::sync::Arc;

use arrow_schema::SchemaRef;
use datafusion::error::Result;
use oxbow::alignment::BamScanner;
use oxbow::{CoordSystem, Select};

use super::super::core::{byte_reader, map_ext, BioBatchIter, BioDriver, BioInput};

fn scanner(header: noodles::sam::Header) -> Result<BamScanner> {
    BamScanner::new(header, Select::All, None, CoordSystem::OneClosed).map_err(map_ext)
}

pub struct BamDriver;

impl BioDriver for BamDriver {
    const FILE_TYPE: &'static str = "bam";

    fn infer_schema(input: &BioInput) -> Result<SchemaRef> {
        let mut reader = noodles::bam::io::Reader::new(byte_reader(input.bytes.clone()));
        let header = reader.read_header().map_err(map_ext)?;
        let scanner = scanner(header)?;
        Ok(Arc::new(scanner.schema().clone()))
    }

    fn scan(input: BioInput, batch_size: usize) -> Result<BioBatchIter> {
        let mut reader = noodles::bam::io::Reader::new(byte_reader(input.bytes));
        let header = reader.read_header().map_err(map_ext)?;
        let scanner = scanner(header)?;
        let batches = scanner
            .scan(reader, None, Some(batch_size), None)
            .map_err(map_ext)?;
        Ok(Box::new(batches))
    }
}
