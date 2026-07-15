//! GFF driver. Mirrors [`super::gtf`]; 8 standard columns, 1-based positions.

use std::sync::Arc;

use arrow_schema::SchemaRef;
use datafusion::error::Result;
use oxbow::gxf::GffScanner;
use oxbow::{CoordSystem, Select};

use super::super::core::{BioBatchIter, BioDriver, BioInput, buf_reader, map_ext};

fn scanner() -> Result<GffScanner> {
    GffScanner::new(None, Select::All, None, CoordSystem::OneClosed).map_err(map_ext)
}

pub struct GffDriver;

impl BioDriver for GffDriver {
    const FILE_TYPE: &'static str = "gff";

    fn infer_schema(_input: &BioInput) -> Result<SchemaRef> {
        let scanner = scanner()?;
        Ok(Arc::new(scanner.schema().clone()))
    }

    fn scan(input: BioInput, batch_size: usize) -> Result<BioBatchIter> {
        let reader = noodles::gff::io::Reader::new(buf_reader(input.bytes, input.gz));
        let scanner = scanner()?;
        let batches = scanner
            .scan(reader, None, Some(batch_size), None)
            .map_err(map_ext)?;
        Ok(Box::new(batches))
    }
}
