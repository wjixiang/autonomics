//! L3 transform tool: sort a dataset by one or more columns.
//!
//! Wraps [`AetherDataset::sort_by`]. Column names are parsed from the
//! `columns` list; prefix with `-` for descending order (e.g. `"p_value,-beta"`).
//! This is a **per-partition** sort — for a fully global sort use `dataset_sql`
//! with `ORDER BY`.

use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction};
use agentik_sdk::types::ToolResult;
use async_trait::async_trait;
use datalake::DatasetStore;
use serde::{Deserialize, Serialize};

use crate::common::err;

pub struct DatasetSortTool {
    pub store: Arc<DatasetStore>,
}

#[derive(Debug, Deserialize, Serialize, agentik_proc::ToolInput)]
#[tool(
    name = "dataset_sort",
    description = "Sort a registered dataset by column(s). Prefix a column with `-` for descending (e.g. [\"p_value\",\"-beta\"]). This is a per-partition sort; for a fully global sort use dataset_sql with ORDER BY."
)]
pub struct DatasetSortInput {
    #[desc = "Name of the registered dataset to sort"]
    pub name: String,
    #[desc = "Column names to sort by. Prefix with `-` for descending, e.g. [\"p_value\",\"-beta\"]."]
    pub columns: Vec<String>,
    #[desc = "Output dataset name. Defaults to the input name (in-place replace)."]
    pub output: Option<String>,
}

#[async_trait]
impl ToolFunction for DatasetSortTool {
    type Input = DatasetSortInput;

    fn timeout_seconds(&self) -> u64 { 300 }

    async fn run(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let name = input.name.trim();
        if name.is_empty() || input.columns.is_empty() {
            return Ok(ToolResult::error("'name' and at least one column are required"));
        }

        // Parse "-col" → (col, false), "col" → (col, true).
        let sort_spec: Vec<(String, bool)> = input
            .columns
            .iter()
            .map(|s| {
                let trimmed = s.trim();
                if let Some(col) = trimmed.strip_prefix('-') {
                    (col.to_string(), false)
                } else {
                    (trimmed.to_string(), true)
                }
            })
            .collect();

        let refs: Vec<(&str, bool)> = sort_spec.iter().map(|(c, a)| (c.as_str(), *a)).collect();

        let ds = self.store.get(name).await.map_err(err)?;
        let sorted = ds.sort_by(&refs).map_err(err)?;

        let output_name = input.output.as_deref().map(str::trim).unwrap_or(name);
        let renamed = datalake::AetherDataset::with_schema(
            output_name,
            sorted.schema().clone(),
            sorted.batches().to_vec(),
        );
        self.store.put_overwrite(renamed).await;

        Ok(ToolResult::success_json(serde_json::json!({
            "dataset": output_name,
            "row_count": self.store.get(output_name).await.map_err(err)?.row_count(),
            "sort_by": sort_spec.iter().map(|(c, a)| if *a { format!("{} ASC", c) } else { format!("{} DESC", c) }).collect::<Vec<_>>(),
        })))
    }
}
