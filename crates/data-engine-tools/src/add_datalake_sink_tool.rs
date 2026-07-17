use std::sync::Arc;

use agentik_sdk::types::ToolResult;
use async_trait::async_trait;
use data_engine::data_engine::{Sink, SinkMode};
use data_engine::runtime::DataEngineClient;
use datalake::Datalake;

use agentik_core::tools::{ToolError, ToolFunction};
use agentik_proc::tool;

use crate::ExecError;

#[tool(
    name = "add_datalake_sink_node",
    description = "Add a sink node to the DAG that writes data to an Iceberg table in the data lake. \
                  The table identifier must be in `namespace.table` form (e.g. `gwas.iris_test`). \
                  \
                  Use mode=append to add rows to an existing table or mode=overwrite \
                  (the default) to replace its contents. \
                  \
                  To write to a local file (CSV or Parquet), use `add_file_sink_node` instead."
)]
pub struct AddDatalakeSinkNodeInput {
    #[desc = "Unique identifier for this node in the DAG"]
    pub id: String,
    #[desc = "Iceberg table identifier in `namespace.table` form \
              (e.g. `gwas.iris_test`). Do NOT include the `iceberg://` URI scheme."]
    pub table: String,
    #[desc = "Write mode: `append` (add rows to the existing table) or `overwrite` (replace its contents). Defaults to `overwrite`."]
    pub mode: Option<String>,
}

pub struct AddDatalakeSinkNodeTool {
    client: Arc<DataEngineClient>,
    datalake: Arc<Datalake>,
}

impl AddDatalakeSinkNodeTool {
    pub fn new(client: Arc<DataEngineClient>, datalake: Arc<Datalake>) -> Self {
        Self { client, datalake }
    }
}

#[async_trait]
impl ToolFunction for AddDatalakeSinkNodeTool {
    type Input = AddDatalakeSinkNodeInput;

    async fn run(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let ident = input.table.trim();
        if ident.is_empty() {
            return Err(ToolError::ExecutionFailed {
                source: Box::new(ExecError::Format(
                    "table identifier must not be empty (expected `namespace.table`)".into(),
                )),
            });
        }
        if ident.contains('/') {
            return Err(ToolError::ExecutionFailed {
                source: Box::new(ExecError::Format(format!(
                    "table identifier must be in `namespace.table` form, got `{ident}`"
                ))),
            });
        }

        let mode = input.mode.as_deref().map(parse_sink_mode).transpose()?;

        self.client
            .add_sink_node(
                input.id,
                Sink::Iceberg {
                    ident: ident.to_string(),
                },
                mode.unwrap_or_default(),
                self.datalake.clone(),
            )
            .await
            .map_err(ExecError::from)?;

        Ok(ToolResult::success("datalake sink node added to DAG"))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sink_mode() {
        assert!(matches!(parse_sink_mode("append"), Ok(SinkMode::Append)));
        assert!(matches!(
            parse_sink_mode("Overwrite"),
            Ok(SinkMode::Overwrite)
        ));
        assert!(parse_sink_mode("truncate").is_err());
    }
}
