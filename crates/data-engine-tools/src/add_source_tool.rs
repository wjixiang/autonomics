use std::sync::Arc;

use agentik_sdk::types::ToolResult;
use async_trait::async_trait;
use data_engine::data_engine::{FileFormat, Source};
use data_engine::runtime::DataEngineClient;

use agentik_core::tools::{ToolError, ToolFunction};
use agentik_proc::tool;

use crate::ExecError;

#[tool(
    name = "add_source_node",
    description = "Add a source node to the DAG that loads data from a file or Iceberg table. \
                  For local files, the format is auto-detected from the extension if not specified. \
                  For Iceberg tables, use the 'iceberg://' URI scheme (e.g. 'iceberg://genetics.ld_score.ukbb_eur')."
)]
pub struct AddSourceNodeInput {
    #[desc = "Unique identifier for this node in the DAG"]
    pub id: String,
    #[desc = "Path to the data file (CSV, Parquet, VCF, etc.) or Iceberg URI (iceberg://namespace.table)."]
    pub path: String,
    #[desc = "Explicit file format (csv, parquet, vcf, etc.). Auto-detected from path if omitted. Ignored for Iceberg URIs."]
    pub format: Option<String>,
}

pub struct AddSourceNodeTool {
    client: Arc<DataEngineClient>,
}

impl AddSourceNodeTool {
    pub fn new(client: Arc<DataEngineClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl ToolFunction for AddSourceNodeTool {
    type Input = AddSourceNodeInput;

    async fn run(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let source = if let Some(ident) = input.path.strip_prefix("iceberg://") {
            Source::Iceberg {
                ident: ident.to_string(),
            }
        } else {
            let format = input.format.map(|f| parse_file_format(&f)).transpose()?;

            Source::File {
                path: input.path,
                format,
            }
        };

        self.client
            .add_source_node(input.id, source)
            .await
            .map_err(ExecError::from)?;

        Ok(ToolResult::success("source node added to DAG"))
    }
}

fn parse_file_format(s: &str) -> std::result::Result<FileFormat, String> {
    match s.to_lowercase().as_str() {
        "csv" => Ok(FileFormat::Csv),
        "parquet" => Ok(FileFormat::Parquet),
        "vcf" => Ok(FileFormat::Vcf),
        "bcf" => Ok(FileFormat::Bcf),
        "fasta" => Ok(FileFormat::Fasta),
        "fa" => Ok(FileFormat::Fasta),
        "fastq" => Ok(FileFormat::Fastq),
        "fq" => Ok(FileFormat::Fastq),
        "bed" => Ok(FileFormat::Bed),
        "gtf" => Ok(FileFormat::Gtf),
        "gff" | "gff3" => Ok(FileFormat::Gff),
        "sam" => Ok(FileFormat::Sam),
        "bam" => Ok(FileFormat::Bam),
        "cram" => Ok(FileFormat::Cram),
        "bigwig" | "bw" => Ok(FileFormat::BigWig),
        "bigbed" | "bb" => Ok(FileFormat::BigBed),
        other => Err(format!("unknown format: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_file_format() {
        assert!(matches!(parse_file_format("csv"), Ok(FileFormat::Csv)));
        assert!(matches!(
            parse_file_format("Parquet"),
            Ok(FileFormat::Parquet)
        ));
        assert!(matches!(parse_file_format("VCF"), Ok(FileFormat::Vcf)));
        assert!(parse_file_format("xyz").is_err());
    }
}
