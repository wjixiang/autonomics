use std::sync::Arc;

use agentik_sdk::types::ToolResult;
use async_trait::async_trait;
use data_engine::data_engine::{Sink, SinkMode, WriteFormat};
use data_engine::runtime::DataEngineClient;
use datalake::Datalake;

use agentik_core::tools::{ToolError, ToolFunction};
use agentik_proc::tool;

use crate::ExecError;

#[tool(
    name = "add_file_sink_node",
    description = "Add a sink node to the DAG that writes data to a local file (CSV or Parquet). \
                  The file format is auto-detected from the path extension (.csv, .parquet, .pq) \
                  if not explicitly specified. \
                  Use mode=append to add rows to an existing file or mode=overwrite \
                  (the default) to replace it. \
                  \
                  To write to the data lake (Iceberg table), use `add_datalake_sink_node` instead."
)]
pub struct AddFileSinkNodeInput {
    #[desc = "Unique identifier for this node in the DAG"]
    pub id: String,
    #[desc = "Output file path. The format is inferred from the extension: \
              `.csv` -> CSV, `.parquet` or `.pq` -> Parquet. \
              If the extension is missing or unrecognized, set `format` explicitly."]
    pub path: String,
    #[desc = "Explicit file format: `csv` or `parquet`. \
              Auto-detected from the path extension if omitted."]
    pub format: Option<String>,
    #[desc = "Write mode: `append` (add rows to an existing file) or `overwrite` (replace it). Defaults to `overwrite`."]
    pub mode: Option<String>,
}

pub struct AddFileSinkNodeTool {
    client: Arc<DataEngineClient>,
    datalake: Arc<Datalake>,
}

impl AddFileSinkNodeTool {
    pub fn new(client: Arc<DataEngineClient>, datalake: Arc<Datalake>) -> Self {
        Self { client, datalake }
    }
}

#[async_trait]
impl ToolFunction for AddFileSinkNodeTool {
    type Input = AddFileSinkNodeInput;

    async fn run(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let format = input
            .format
            .as_deref()
            .map(parse_file_format)
            .transpose()?
            .or_else(|| detect_file_format(&input.path))
            .unwrap_or(WriteFormat::Csv);

        let sink = Sink::File {
            path: input.path.clone(),
            format,
        };
        let mode = input.mode.as_deref().map(parse_sink_mode).transpose()?;

        self.client
            .add_sink_node(
                input.id,
                sink,
                mode.unwrap_or_default(),
                self.datalake.clone(),
            )
            .await
            .map_err(ExecError::from)?;

        Ok(ToolResult::success("file sink node added to DAG"))
    }
}

fn parse_file_format(s: &str) -> std::result::Result<WriteFormat, String> {
    match s.to_lowercase().as_str() {
        "csv" => Ok(WriteFormat::Csv),
        "parquet" => Ok(WriteFormat::Parquet),
        other => Err(format!(
            "unknown file format: {other} (supported: csv, parquet)"
        )),
    }
}

fn parse_sink_mode(s: &str) -> std::result::Result<SinkMode, String> {
    match s.to_lowercase().as_str() {
        "append" => Ok(SinkMode::Append),
        "overwrite" => Ok(SinkMode::Overwrite),
        other => Err(format!(
            "unknown sink mode: {other} (supported: append, overwrite)"
        )),
    }
}

fn detect_file_format(path: &str) -> Option<WriteFormat> {
    match path.rsplit('.').next()?.to_lowercase().as_str() {
        "csv" => Some(WriteFormat::Csv),
        "parquet" | "pq" => Some(WriteFormat::Parquet),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_file_format() {
        assert!(matches!(parse_file_format("csv"), Ok(WriteFormat::Csv)));
        assert!(matches!(
            parse_file_format("Parquet"),
            Ok(WriteFormat::Parquet)
        ));
        assert!(parse_file_format("iceberg").is_err());
        assert!(parse_file_format("json").is_err());
    }

    #[test]
    fn test_parse_sink_mode() {
        assert!(matches!(parse_sink_mode("append"), Ok(SinkMode::Append)));
        assert!(matches!(
            parse_sink_mode("Overwrite"),
            Ok(SinkMode::Overwrite)
        ));
        assert!(parse_sink_mode("truncate").is_err());
    }

    #[test]
    fn test_detect_file_format() {
        assert_eq!(detect_file_format("out.csv"), Some(WriteFormat::Csv));
        assert_eq!(detect_file_format("out.parquet"), Some(WriteFormat::Parquet));
        assert_eq!(detect_file_format("out.pq"), Some(WriteFormat::Parquet));
        assert_eq!(detect_file_format("out.txt"), None);
        assert_eq!(detect_file_format("noext"), None);
        // Iceberg URI / table identifiers must NOT be auto-detected as a file format.
        assert_eq!(detect_file_format("iceberg://gwas.iris_test"), None);
        assert_eq!(detect_file_format("gwas.iris_test"), None);
    }
}