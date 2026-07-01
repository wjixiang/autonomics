//! L4 analysis tool: ordinary least squares (OLS) regression.
//!
//! Extracts numeric columns via [`Dataset::extract_f64_columns`],
//! then calls [`stat_primitives::regression::ols`]. The result dataset
//! contains one row with the regression coefficients, standard errors, t-stats,
//! p-values, R², etc.

use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction};
use agentik_sdk::types::ToolResult;
use async_trait::async_trait;
use data_engine::{DatasetStore, NullPolicy};
use serde::{Deserialize, Serialize};

use crate::common::err;

pub struct DatasetOlsTool {
    pub store: Arc<DatasetStore>,
}

#[derive(Debug, Deserialize, Serialize, agentik_proc::ToolInput)]
#[tool(
    name = "dataset_ols",
    description = "Run OLS regression on a registered dataset. Specify the dependent variable (y_column) and predictor columns. Returns coefficients, standard errors, t-stats, p-values, R-squared, and residual stats. Null values are dropped before computation. The result is stored as a new dataset (default name: ols_{y_column})."
)]
pub struct DatasetOlsInput {
    #[desc = "Name of the registered dataset"]
    pub name: String,
    #[desc = "Dependent (outcome) variable column"]
    pub y_column: String,
    #[desc = "Independent (predictor) columns"]
    pub predictors: Vec<String>,
    #[desc = "Include an intercept term. Defaults to true."]
    pub intercept: Option<bool>,
    #[desc = "Output dataset name. Defaults to 'ols_{y_column}'."]
    pub output: Option<String>,
}

#[async_trait]
impl ToolFunction for DatasetOlsTool {
    type Input = DatasetOlsInput;

    fn timeout_seconds(&self) -> u64 { 300 }

    async fn run(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let name = input.name.trim();
        let y_col = input.y_column.trim();
        if name.is_empty() || y_col.is_empty() || input.predictors.is_empty() {
            return Ok(ToolResult::error(
                "'name', 'y_column', and at least one predictor are required",
            ));
        }
        let intercept = input.intercept.unwrap_or(true);

        let default_name = format!("ols_{y_col}");
        let output_name = input
            .output
            .as_deref()
            .map(str::trim)
            .unwrap_or(&default_name);

        let ds = self.store.get(name).await.map_err(err)?;

        // Extract all columns needed: y + predictors.
        let all_cols: Vec<&str> = std::iter::once(y_col)
            .chain(input.predictors.iter().map(|s| s.trim()))
            .collect();
        let columns = ds.extract_f64_columns(&all_cols, &NullPolicy::DropNulls).map_err(err)?;

        if columns.is_empty() || columns.iter().any(|c| c.is_empty()) {
            return Ok(ToolResult::error(
                "all specified columns must contain at least one non-null value",
            ));
        }
        if columns.iter().any(|c| c.len() != columns[0].len()) {
            return Ok(ToolResult::error(
                "columns have different row counts after dropping nulls — check for mismatched null patterns across columns",
            ));
        }

        // Build predictor slices: [&predictor1, &predictor2, ...]
        let predictor_slices: Vec<&[f64]> = columns[1..].iter().map(|c| c.as_slice()).collect();
        let y_slice: &[f64] = &columns[0];

        let reg = stat_primitives::regression::ols(&predictor_slices, y_slice, intercept)
            .map_err(err)?;

        Ok(ToolResult::success_json(serde_json::json!({
            "dataset": output_name,
            "method": "OLS",
            "y_column": y_col,
            "predictors": input.predictors.iter().map(|s| s.trim()).collect::<Vec<_>>(),
            "intercept": intercept,
            "n_obs": reg.n_obs,
            "n_params": reg.n_params,
            "df_residual": reg.df_residual,
            "r_squared": reg.r_squared,
            "adj_r_squared": reg.adj_r_squared,
            "coefficients": reg.coefficients,
            "std_errors": reg.std_errors,
            "t_stats": reg.t_stats,
            "p_values": reg.p_values,
        })))
    }
}
