//! FASTQ driver. Header-less; schema is determined by the scanner config.

use std::sync::Arc;

use arrow_schema::SchemaRef;
use datafusion::error::Result;
use oxbow::Select;
use oxbow::sequence::FastqScanner;

use super::super::core::{BioBatchIter, BioDriver, BioInput, buf_reader, map_ext};

fn scanner() -> Result<FastqScanner> {
    FastqScanner::new(Select::All).map_err(map_ext)
}

pub struct FastqDriver;

impl BioDriver for FastqDriver {
    const FILE_TYPE: &'static str = "fastq";

    fn infer_schema(_input: &BioInput) -> Result<SchemaRef> {
        let scanner = scanner()?;
        Ok(Arc::new(scanner.schema().clone()))
    }

    fn scan(input: BioInput, batch_size: usize) -> Result<BioBatchIter> {
        let reader = noodles::fastq::io::Reader::new(buf_reader(input.bytes, input.gz));
        let scanner = scanner()?;
        let batches = scanner
            .scan(reader, None, Some(batch_size), None)
            .map_err(map_ext)?;
        Ok(Box::new(batches))
    }
}
