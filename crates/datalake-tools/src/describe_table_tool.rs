use std::sync::Arc;

use async_trait::async_trait;
use datalake::Datalake;

use agentik_core::tools::{ToolError, ToolFunction};
use agentik_proc::tool;
use agentik_sdk::types::ToolResult;

#[tool(
    name = "describe_iceberg_table",
    description = "Show the schema (columns, Iceberg data types, nullability, field ids, docs) \
                  of a specific table in the Iceberg data lake. Pass the full dotted identifier \
                  as 'namespace.table' — nested namespaces use dots, e.g. \
                  'genetics.ld_score.ukbb_eur'. Reads the schema directly from the Iceberg \
                  catalog (not DataFusion), so multi-level namespaces are fully supported."
)]
pub struct DescribeIcebergTableInput {
    #[desc = "Full dotted table identifier, e.g. 'genetics.ld_score.ukbb_eur' or 'gwas.gwas_study'"]
    pub table: String,
}

pub struct DescribeIcebergTableTool {
    datalake: Arc<Datalake>,
}

impl DescribeIcebergTableTool {
    pub fn new(datalake: Arc<Datalake>) -> Self {
        Self { datalake }
    }
}

#[async_trait]
impl ToolFunction for DescribeIcebergTableTool {
    type Input = DescribeIcebergTableInput;

    fn timeout_seconds(&self) -> u64 {
        120
    }

    async fn run(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let table = input.table.trim();
        if table.is_empty() {
            return Ok(ToolResult::error("table identifier must not be empty"));
        }

        let fields = self
            .datalake
            .table_schema(table)
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                source: format!("failed to describe {table}: {e}").into(),
            })?;

        let columns: Vec<serde_json::Value> = fields
            .iter()
            .map(|f| {
                serde_json::json!({
                    "name": f.name,
                    "type": f.field_type.to_string(),
                    "nullable": !f.required,
                    "field_id": f.id,
                    "doc": f.doc,
                })
            })
            .collect();

        Ok(ToolResult::success_json(serde_json::json!({
            "table": table,
            "columns": columns,
            "column_count": columns.len(),
        })))
    }
}
