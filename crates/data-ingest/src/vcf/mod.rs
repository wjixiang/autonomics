//! VCF format ingestion.

pub mod reader;
pub mod schema;

pub use reader::VcfIngestor;
pub use schema::{vcf_arrow_schema, vcf_fixed_schema};
