use std::sync::Arc;

use agentik_sdk::types::ToolResult;
use async_trait::async_trait;
use data_engine::data_engine::{Sink, WriteFormat};
use data_engine::runtime::DataEngineClient;

use agentik_core::tools::{ToolError, ToolFunction};
use agentik_proc::tool;

use crate::ExecError;

#[tool(
    name = "add_sink_node",
    description = "Add a sink node to the DAG that writes data to a file. \
                  The file format is auto-detected from the extension if not specified."
)]
pub struct AddSinkNodeInput {
    #[desc = "Unique identifier for this node in the DAG"]
    pub id: String,
    #[desc = "Output file path (CSV, Parquet, etc.)"]
    pub path: String,
    #[desc = "Explicit write format (csv, parquet). Auto-detected from path if omitted."]
    pub format: Option<String>,
}

pub struct AddSinkNodeTool {
    client: Arc<DataEngineClient>,
}

impl AddSinkNodeTool {
    pub fn new(client: Arc<DataEngineClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl ToolFunction for AddSinkNodeTool {
    type Input = AddSinkNodeInput;

    async fn run(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let format = input.format.map(|f| parse_write_format(&f)).transpose()?;

        // Auto-detect from extension when not explicitly specified
        let format = format.or_else(|| detect_write_format(&input.path));

        let sink = Sink::File {
            path: input.path,
            format: format.unwrap_or(WriteFormat::Csv),
        };

        self.client
            .add_sink_node(input.id, sink)
            .await
            .map_err(ExecError::from)?;

        Ok(ToolResult::success("sink node added to DAG"))
    }
}

fn parse_write_format(s: &str) -> std::result::Result<WriteFormat, String> {
    match s.to_lowercase().as_str() {
        "csv" => Ok(WriteFormat::Csv),
        "parquet" => Ok(WriteFormat::Parquet),
        other => Err(format!(
            "unknown write format: {other} (supported: csv, parquet)"
        )),
    }
}

fn detect_write_format(path: &str) -> Option<WriteFormat> {
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
    fn test_parse_write_format() {
        assert!(matches!(parse_write_format("csv"), Ok(WriteFormat::Csv)));
        assert!(matches!(
            parse_write_format("Parquet"),
            Ok(WriteFormat::Parquet)
        ));
        assert!(parse_write_format("json").is_err());
    }

    #[test]
    fn test_detect_write_format() {
        assert_eq!(detect_write_format("out.csv"), Some(WriteFormat::Csv));
        assert_eq!(
            detect_write_format("out.parquet"),
            Some(WriteFormat::Parquet)
        );
        assert_eq!(detect_write_format("out.pq"), Some(WriteFormat::Parquet));
        assert_eq!(detect_write_format("out.txt"), None);
        assert_eq!(detect_write_format("noext"), None);
    }
}
