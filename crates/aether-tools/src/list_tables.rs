//! Aether tool: list all Iceberg tables visible through the DataFusion catalog.

use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction};
use agentik_sdk::types::ToolResult;
use async_trait::async_trait;
use datalake::aether::AetherWorkspace;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, agentik_proc::ToolInput)]
#[tool(
    name = "iceberg_list_tables",
    description = "List all Iceberg tables visible in the DataFusion workspace. Returns table names grouped by namespace. Optionally filter to a specific namespace."
)]
pub struct IcebergListTablesInput {
    #[desc = "Optional namespace (schema) to filter tables by. If omitted, lists all tables across all namespaces."]
    pub namespace: Option<String>,
}

pub struct IcebergListTablesTool {
    pub workspace: Arc<AetherWorkspace>,
}

#[async_trait]
impl ToolFunction for IcebergListTablesTool {
    type Input = IcebergListTablesInput;

    async fn run(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let workspace = self.workspace.clone();
        let result = match input
            .namespace
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(ns) => {
                let tables = workspace
                    .list_tables_in_namespace(ns)
                    .map_err(|e| ToolError::ExecutionFailed { source: e.into() })?;
                serde_json::json!({
                    "namespace": ns,
                    "tables": tables,
                    "count": tables.len(),
                })
            }
            None => {
                let all_tables = workspace
                    .list_tables()
                    .map_err(|e| ToolError::ExecutionFailed { source: e.into() })?;

                let mut grouped: std::collections::BTreeMap<String, Vec<String>> =
                    std::collections::BTreeMap::new();
                for (namespace, table) in all_tables {
                    grouped.entry(namespace).or_default().push(table);
                }

                let namespaces: Vec<serde_json::Value> = grouped
                    .into_iter()
                    .map(|(ns, tables)| {
                        serde_json::json!({
                            "namespace": ns,
                            "tables": tables,
                            "count": tables.len(),
                        })
                    })
                    .collect();

                let total: u64 = namespaces
                    .iter()
                    .map(|n| n["count"].as_u64().unwrap_or(0))
                    .sum();

                serde_json::json!({
                    "namespaces": namespaces,
                    "total_tables": total,
                })
            }
        };

        Ok(ToolResult::success_json(result))
    }
}
