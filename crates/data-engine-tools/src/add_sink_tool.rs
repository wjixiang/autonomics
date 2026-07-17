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
    name = "add_sink_node",
    description = "Add a sink node to the DAG that writes data to a file. \
                  The file format is auto-detected from the extension if not specified. \
                  Use mode=append to add rows to an existing file or mode=overwrite \
                  (the default) to replace it."
)]
pub struct AddSinkNodeInput {
    #[desc = "Unique identifier for this node in the DAG"]
    pub id: String,
    #[desc = "Output file path (CSV, Parquet, etc.)"]
    pub path: String,
    #[desc = "Explicit write format (csv, parquet, iceberg). Auto-detected from path if omitted."]
    pub format: Option<String>,
    #[desc = "Write mode: `append` (add rows to an existing file) or `overwrite` (replace it). Defaults to `overwrite`."]
    pub mode: Option<String>,
}

pub struct AddSinkNodeTool {
    client: Arc<DataEngineClient>,
    datalake: Arc<Datalake>,
}

impl AddSinkNodeTool {
    pub fn new(client: Arc<DataEngineClient>, datalake: Arc<Datalake>) -> Self {
        Self { client, datalake }
    }
}

#[async_trait]
impl ToolFunction for AddSinkNodeTool {
    type Input = AddSinkNodeInput;

    async fn run(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let explicit = input.format.as_deref().map(parse_sink_format).transpose()?;

        // Auto-detect from path when not explicitly specified
        let format = explicit.or_else(|| detect_sink_format(&input.path));

        let sink = match format.unwrap_or(SinkFormat::File(WriteFormat::Csv)) {
            SinkFormat::File(wf) => Sink::File {
                path: input.path.clone(),
                format: wf,
            },
            SinkFormat::Iceberg => {
                let ident = input
                    .path
                    .strip_prefix("iceberg://")
                    .unwrap_or(&input.path)
                    .to_string();
                Sink::Iceberg { ident }
            }
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

        Ok(ToolResult::success("sink node added to DAG"))
    }
}

/// Resolved sink target: a file format or an Iceberg table.
#[derive(Debug, PartialEq, Eq)]
enum SinkFormat {
    File(WriteFormat),
    Iceberg,
}

fn parse_sink_format(s: &str) -> std::result::Result<SinkFormat, String> {
    match s.to_lowercase().as_str() {
        "csv" => Ok(SinkFormat::File(WriteFormat::Csv)),
        "parquet" => Ok(SinkFormat::File(WriteFormat::Parquet)),
        "iceberg" => Ok(SinkFormat::Iceberg),
        other => Err(format!(
            "unknown write format: {other} (supported: csv, parquet, iceberg)"
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

fn detect_sink_format(path: &str) -> Option<SinkFormat> {
    if path.starts_with("iceberg://") {
        return Some(SinkFormat::Iceberg);
    }
    match path.rsplit('.').next()?.to_lowercase().as_str() {
        "csv" => Some(SinkFormat::File(WriteFormat::Csv)),
        "parquet" | "pq" => Some(SinkFormat::File(WriteFormat::Parquet)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sink_format() {
        assert!(matches!(
            parse_sink_format("csv"),
            Ok(SinkFormat::File(WriteFormat::Csv))
        ));
        assert!(matches!(
            parse_sink_format("Parquet"),
            Ok(SinkFormat::File(WriteFormat::Parquet))
        ));
        assert!(matches!(
            parse_sink_format("Iceberg"),
            Ok(SinkFormat::Iceberg)
        ));
        assert!(parse_sink_format("json").is_err());
    }

    #[test]
    fn test_sink_mode() {
        assert!(matches!(parse_sink_mode("append"), Ok(SinkMode::Append)));
        assert!(matches!(
            parse_sink_mode("Overwrite"),
            Ok(SinkMode::Overwrite)
        ));
        assert!(parse_sink_mode("truncate").is_err());
    }

    #[test]
    fn test_detect_sink_format() {
        assert_eq!(
            detect_sink_format("out.csv"),
            Some(SinkFormat::File(WriteFormat::Csv))
        );
        assert_eq!(
            detect_sink_format("out.parquet"),
            Some(SinkFormat::File(WriteFormat::Parquet))
        );
        assert_eq!(
            detect_sink_format("out.pq"),
            Some(SinkFormat::File(WriteFormat::Parquet))
        );
        assert_eq!(detect_sink_format("out.txt"), None);
        assert_eq!(detect_sink_format("noext"), None);
        assert_eq!(
            detect_sink_format("iceberg://gwas.iris_test"),
            Some(SinkFormat::Iceberg)
        );
    }
}
