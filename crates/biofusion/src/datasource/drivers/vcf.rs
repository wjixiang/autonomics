//! VCF driver.

use std::sync::Arc;

use arrow_schema::SchemaRef;
use datafusion::error::Result;
use oxbow::variant::VcfScanner;
use oxbow::{CoordSystem, Select};

use super::super::core::{map_ext, buf_reader, BioBatchIter, BioDriver, BioInput};

/// Build an oxbow [`VcfScanner`] selecting every contig / field / sample
/// (full schema). Shared by schema inference and the runtime scan so they
/// agree on the produced schema.
fn scanner(header: noodles::vcf::Header) -> Result<VcfScanner> {
    VcfScanner::new(
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

pub struct VcfDriver;

impl BioDriver for VcfDriver {
    const FILE_TYPE: &'static str = "vcf";

    fn infer_schema(input: &BioInput) -> Result<SchemaRef> {
        let mut reader = noodles::vcf::io::Reader::new(buf_reader(input.bytes.clone(), input.gz));
        let header = reader.read_header().map_err(map_ext)?;
        let scanner = scanner(header)?;
        Ok(Arc::new(scanner.schema().clone()))
    }

    fn scan(input: BioInput, batch_size: usize) -> Result<BioBatchIter> {
        let mut reader = noodles::vcf::io::Reader::new(buf_reader(input.bytes, input.gz));
        let header = reader.read_header().map_err(map_ext)?;
        let scanner = scanner(header)?;
        let batches = scanner
            .scan(reader, None, Some(batch_size), None)
            .map_err(map_ext)?;
        Ok(Box::new(batches))
    }
}
