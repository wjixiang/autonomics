//! Read-side tool: preview Iceberg table rows without materializing a dataset.
//!
//! This is the unified Iceberg peek tool — it runs a read-only `SELECT` against
//! an Iceberg table and returns the first N rows plus the column schema, but
//! does NOT register any dataset in the store. It is the read-only inspection
//! counterpart to `dataset_load_table` (which materializes into working memory).
//!
//! Note: the DataFusion catalog provider registers only top-level Iceberg
//! namespaces as schemas, so `namespace` here should be a single top-level
//! segment (e.g. `analytics`, not `warehouse.analytics`).

use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction};
use agentik_sdk::types::ToolResult;
use arrow::util::pretty::pretty_format_batches;
use async_trait::async_trait;
use datalake::aether::AetherWorkspace;
use serde::{Deserialize, Serialize};

use crate::common::err;

/// Quote a SQL identifier with double quotes, escaping embedded quotes.
fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

#[derive(Debug, Deserialize, Serialize, agentik_proc::ToolInput)]
#[tool(
    name = "iceberg_preview_table",
    description = "Preview rows directly from an Iceberg table WITHOUT loading them into the store. Returns the first N rows as a pretty-printed table plus the column schema. Use this for read-only inspection — to load data for analysis, use dataset_load_table instead. The namespace must be a single top-level segment (e.g. 'analytics')."
)]
pub struct IcebergPreviewTableInput {
    #[desc = "Top-level namespace segment containing the table, e.g. 'analytics'"]
    pub namespace: String,
    #[desc = "Iceberg table name to preview"]
    pub table: String,
    #[desc = "Optional list of columns to project. Defaults to all columns (*)"]
    pub columns: Option<Vec<String>>,
    #[desc = "Optional SQL WHERE clause (without the 'WHERE' keyword), e.g. \"event = 'login'\""]
    pub where_clause: Option<String>,
    #[desc = "Maximum number of rows to return. Defaults to 50."]
    pub limit: Option<usize>,
}

pub struct IcebergPreviewTableTool {
    pub workspace: Arc<AetherWorkspace>,
}

#[async_trait]
impl ToolFunction for IcebergPreviewTableTool {
    type Input = IcebergPreviewTableInput;

    /// Iceberg preview can involve large scans; allow up to 5 minutes.
    fn timeout_seconds(&self) -> u64 {
        300
    }

    async fn run(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let namespace = input.namespace.trim();
        let table = input.table.trim();
        if namespace.is_empty() || table.is_empty() {
            return Ok(ToolResult::error(
                "both 'namespace' and 'table' are required",
            ));
        }

        let projection = match input.columns.as_ref() {
            Some(cols) if !cols.is_empty() => cols
                .iter()
                .map(|c| quote_ident(c.trim()))
                .collect::<Vec<_>>()
                .join(", "),
            _ => "*".to_string(),
        };

        // Dotted 3-part form: catalog.schema.table — the comma form is a SQL
        // cross-join and resolves under the wrong catalog.
        let mut sql = format!(
            "SELECT {projection} FROM {}.{}.{}",
            quote_ident("iceberg"),
            quote_ident(namespace),
            quote_ident(table),
        );
        if let Some(clause) = input
            .where_clause
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            sql.push_str(" WHERE ");
            sql.push_str(clause);
        }
        let limit = input.limit.unwrap_or(50);
        sql.push_str(&format!(" LIMIT {limit}"));

        let ctx = self.workspace.ctx();

        let df = ctx.sql(&sql).await.map_err(err)?;

        // Extract schema before collecting (DF ownership rules).
        let schema = df.schema().as_arrow();
        let schema_json: Vec<serde_json::Value> = schema
            .fields()
            .iter()
            .map(|f| {
                serde_json::json!({
                    "name": f.name(),
                    "type": f.data_type().to_string(),
                    "nullable": f.is_nullable(),
                })
            })
            .collect();

        let batches = df.collect().await.map_err(err)?;
        let row_count: usize = batches.iter().map(|b| b.num_rows()).sum();
        let pretty = pretty_format_batches(&batches)
            .map_err(err)?
            .to_string();

        Ok(ToolResult::success_json(serde_json::json!({
            "source": format!("{namespace}.{table}"),
            "schema": schema_json,
            "row_count": row_count,
            "limit": limit,
            "truncated": row_count >= limit,
            "materialized": false,
            "rows": pretty,
        })))
    }
}
