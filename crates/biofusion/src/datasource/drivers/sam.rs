//! SAM driver.
//!
//! The SAM header (embedded at the top of the file) drives both schema
//! inference and the scan. Positions are 1-based.

use std::sync::Arc;

use arrow_schema::SchemaRef;
use datafusion::error::Result;
use oxbow::alignment::SamScanner;
use oxbow::{CoordSystem, Select};

use super::super::core::{BioBatchIter, BioDriver, BioInput, buf_reader, map_ext};

fn scanner(header: noodles::sam::Header) -> Result<SamScanner> {
    SamScanner::new(header, Select::All, None, CoordSystem::OneClosed).map_err(map_ext)
}

pub struct SamDriver;

impl BioDriver for SamDriver {
    const FILE_TYPE: &'static str = "sam";

    fn infer_schema(input: &BioInput) -> Result<SchemaRef> {
        let mut reader = noodles::sam::io::Reader::new(buf_reader(input.bytes.clone(), input.gz));
        let header = reader.read_header().map_err(map_ext)?;
        let scanner = scanner(header)?;
        Ok(Arc::new(scanner.schema().clone()))
    }

    fn scan(input: BioInput, batch_size: usize) -> Result<BioBatchIter> {
        let mut reader = noodles::sam::io::Reader::new(buf_reader(input.bytes, input.gz));
        let header = reader.read_header().map_err(map_ext)?;
        let scanner = scanner(header)?;
        let batches = scanner
            .scan(reader, None, Some(batch_size), None)
            .map_err(map_ext)?;
        Ok(Box::new(batches))
    }
}
