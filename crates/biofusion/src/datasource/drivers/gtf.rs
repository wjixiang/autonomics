//! GTF driver.
//!
//! Header-less. Only the 8 standard GTF columns are produced (`attr_defs =
//! None` → no attributes struct column); positions are 1-based.

use std::sync::Arc;

use arrow_schema::SchemaRef;
use datafusion::error::Result;
use oxbow::gxf::GtfScanner;
use oxbow::{CoordSystem, Select};

use super::super::core::{BioBatchIter, BioDriver, BioInput, buf_reader, map_ext};

fn scanner() -> Result<GtfScanner> {
    GtfScanner::new(None, Select::All, None, CoordSystem::OneClosed).map_err(map_ext)
}

pub struct GtfDriver;

impl BioDriver for GtfDriver {
    const FILE_TYPE: &'static str = "gtf";

    fn infer_schema(_input: &BioInput) -> Result<SchemaRef> {
        let scanner = scanner()?;
        Ok(Arc::new(scanner.schema().clone()))
    }

    fn scan(input: BioInput, batch_size: usize) -> Result<BioBatchIter> {
        let reader = noodles::gtf::io::Reader::new(buf_reader(input.bytes, input.gz));
        let scanner = scanner()?;
        let batches = scanner
            .scan(reader, None, Some(batch_size), None)
            .map_err(map_ext)?;
        Ok(Box::new(batches))
    }
}
