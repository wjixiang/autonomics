//! L1 ingestion tool: load an Iceberg table into a named in-memory dataset.
//!
//! `dataset_load_table` is the primary entry point for getting persistent data
//! into an [`Dataset`]. Data flows from a registered Iceberg table
//! (accessed through the `iceberg` DataFusion catalog) into the
//! [`DatasetStore`]'s working memory.

use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction};
use agentik_sdk::types::ToolResult;
use async_trait::async_trait;
use data_engine::data_session::DataSession;
use data_engine::DatasetStore;
use serde::{Deserialize, Serialize};

use crate::common::err;

#[derive(Debug, Deserialize, Serialize, agentik_proc::ToolInput)]
#[tool(
    name = "dataset_load_table",
    description = "Load rows from an Iceberg table into a named in-memory dataset for analysis. Optionally project specific columns, filter rows with a SQL WHERE expression, and cap the row count. The namespace must be a single top-level segment (e.g. 'analytics'). The loaded dataset is stored under `name` and can be referenced by subsequent dataset tools."
)]
pub struct DatasetLoadTableInput {
    #[desc = "Name to register the loaded dataset under in the store"]
    pub name: String,
    #[desc = "Top-level Iceberg namespace segment, e.g. 'analytics'"]
    pub namespace: String,
    #[desc = "Iceberg table name to load"]
    pub table: String,
    #[desc = "Optional list of columns to project. Defaults to all columns (*)"]
    pub columns: Option<Vec<String>>,
    #[desc = "Optional SQL WHERE clause (without the 'WHERE' keyword), e.g. \"p_value < 5e-8\""]
    pub filter: Option<String>,
    #[desc = "Optional maximum number of rows to load"]
    pub limit: Option<usize>,
}

pub struct DatasetLoadTableTool {
    pub workspace: Arc<DataSession>,
    pub store: Arc<DatasetStore>,
}

#[async_trait]
impl ToolFunction for DatasetLoadTableTool {
    type Input = DatasetLoadTableInput;

    fn timeout_seconds(&self) -> u64 {
        300
    }

    async fn run(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let name = input.name.trim();
        let namespace = input.namespace.trim();
        let table = input.table.trim();
        if name.is_empty() || namespace.is_empty() || table.is_empty() {
            return Ok(ToolResult::error(
                "'name', 'namespace', and 'table' are all required",
            ));
        }

        let columns: Option<Vec<String>> = input.columns.map(|mut c| {
            c.retain(|s| !s.trim().is_empty());
            c
        });
        let columns_ref: Option<&[String]> = columns.as_deref();

        let ds = self
            .workspace
            .read_table(
                &self.store,
                name,
                namespace,
                table,
                columns_ref,
                input.filter.as_deref(),
                input.limit,
            )
            .await
            .map_err(err)?;

        let schema = ds.schema_json();
        Ok(ToolResult::success_json(serde_json::json!({
            "dataset": name,
            "source": format!("{namespace}.{table}"),
            "row_count": ds.row_count(),
            "column_count": ds.column_count(),
            "schema": schema["columns"],
        })))
    }
}
