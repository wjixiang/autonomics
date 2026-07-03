//! BCF driver.
//!
//! BCF is always BGZF-compressed; [`noodles::bcf::io::Reader::new`] wraps the
//! inner reader in a BGZF decoder itself, so the `gz` flag is ignored.

use std::sync::Arc;

use arrow_schema::SchemaRef;
use datafusion::error::Result;
use oxbow::variant::BcfScanner;
use oxbow::{CoordSystem, Select};

use super::super::core::{byte_reader, map_ext, BioBatchIter, BioDriver, BioInput};

fn scanner(header: noodles::vcf::Header) -> Result<BcfScanner> {
    BcfScanner::new(
        header,
        Select::All,
        Select::All,
        Select::All,
        None,
        Select::All,
        None,
        CoordSystem::OneClosed,
    )
    .map_err(map_ext)
}

pub struct BcfDriver;

impl BioDriver for BcfDriver {
    const FILE_TYPE: &'static str = "bcf";

    fn infer_schema(input: &BioInput) -> Result<SchemaRef> {
        let mut reader = noodles::bcf::io::Reader::new(byte_reader(input.bytes.clone()));
        let header = reader.read_header().map_err(map_ext)?;
        let scanner = scanner(header)?;
        Ok(Arc::new(scanner.schema().clone()))
    }

    fn scan(input: BioInput, batch_size: usize) -> Result<BioBatchIter> {
        let mut reader = noodles::bcf::io::Reader::new(byte_reader(input.bytes));
        let header = reader.read_header().map_err(map_ext)?;
        let scanner = scanner(header)?;
        let batches = scanner
            .scan(reader, None, Some(batch_size), None)
            .map_err(map_ext)?;
        Ok(Box::new(batches))
    }
}
