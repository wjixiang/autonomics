//! L3 transform tool: apply a SQL expression to a column (map/derive).
//!
//! Wraps [`DatasetStore::map_expr`] to give agents a focused interface
//! for column-level transformations without writing full SQL.

use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction};
use agentik_sdk::types::ToolResult;
use async_trait::async_trait;
use datalake::DatasetStore;
use serde::{Deserialize, Serialize};

use crate::common::err;

pub struct DatasetMapTool {
    pub store: Arc<DatasetStore>,
}

#[derive(Debug, Deserialize, Serialize, agentik_proc::ToolInput)]
#[tool(
    name = "dataset_map",
    description = "Apply a SQL expression to transform or derive a column in a registered dataset. The expression can reference any column. Examples: '-log10(p_value)', 'beta * 2', 'abs(z_score)', 'ln(y)'. Set output_col to the same as column to replace in-place, or provide a new name to add a derived column."
)]
pub struct DatasetMapInput {
    #[desc = "Name of the registered dataset (also used as output name)"]
    pub name: String,
    #[desc = "Source column name (for documentation; expression may reference any column)"]
    pub column: String,
    #[desc = "SQL scalar expression, e.g. '-log10(p_value)', 'beta * 2', 'abs(z_score)'"]
    pub expression: String,
    #[desc = "Output column name. Defaults to same as column (in-place replace)."]
    pub output_col: Option<String>,
}

#[async_trait]
impl ToolFunction for DatasetMapTool {
    type Input = DatasetMapInput;

    fn timeout_seconds(&self) -> u64 { 300 }

    async fn run(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let name = input.name.trim();
        let column = input.column.trim();
        let expression = input.expression.trim();
        if name.is_empty() || column.is_empty() || expression.is_empty() {
            return Ok(ToolResult::error(
                "'name', 'column', and 'expression' are all required",
            ));
        }
        let output_col = input
            .output_col
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(column);

        self.store
            .map_expr(name, name, column, expression, output_col)
            .await
            .map_err(err)?;

        let ds = self.store.get(name).await.map_err(err)?;
        Ok(ToolResult::success_json(serde_json::json!({
            "dataset": name,
            "operation": "map",
            "column": column,
            "expression": expression,
            "output_col": output_col,
            "row_count": ds.row_count(),
            "column_count": ds.column_count(),
        })))
    }
}
