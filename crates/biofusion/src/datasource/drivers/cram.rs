//! CRAM driver.
//!
//! CRAM decoding nominally needs a reference-sequence FASTA repository. For
//! the default integration we supply an empty repository, which suffices for
//! CRAM files that embed their own references; files requiring external
//! references will surface a decode error at scan time. Positions are 1-based.

use std::sync::Arc;

use arrow_schema::SchemaRef;
use datafusion::error::Result;
use oxbow::alignment::CramScanner;
use oxbow::{CoordSystem, Select};

use super::super::core::{byte_reader, map_ext, BioBatchIter, BioDriver, BioInput};

/// An empty reference repository (every lookup returns `None`).
fn empty_repo() -> noodles::fasta::Repository {
    noodles::fasta::Repository::new(noodles::fasta::repository::adapters::Empty::new())
}

fn scanner(header: noodles::sam::Header) -> Result<CramScanner> {
    CramScanner::new(header, Select::All, None, empty_repo(), CoordSystem::OneClosed)
        .map_err(map_ext)
}

pub struct CramDriver;

impl BioDriver for CramDriver {
    const FILE_TYPE: &'static str = "cram";

    fn infer_schema(input: &BioInput) -> Result<SchemaRef> {
        let mut reader = noodles::cram::io::Reader::new(byte_reader(input.bytes.clone()));
        let header = reader.read_header().map_err(map_ext)?;
        let scanner = scanner(header)?;
        Ok(Arc::new(scanner.schema().clone()))
    }

    fn scan(input: BioInput, batch_size: usize) -> Result<BioBatchIter> {
        let mut reader = noodles::cram::io::Reader::new(byte_reader(input.bytes));
        let header = reader.read_header().map_err(map_ext)?;
        let scanner = scanner(header)?;
        let batches = scanner
            .scan(reader, None, Some(batch_size), None)
            .map_err(map_ext)?;
        Ok(Box::new(batches))
    }
}
