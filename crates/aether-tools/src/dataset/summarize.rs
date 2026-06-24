//! L4 analysis tool: descriptive statistics for a numeric column.
//!
//! Extracts a numeric column via [`AetherDataset::extract_f64`] and computes
//! descriptive statistics using `stat_primitives`.

use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction};
use agentik_sdk::types::ToolResult;
use async_trait::async_trait;
use datalake::{DatasetStore, NullPolicy};
use serde::{Deserialize, Serialize};

use crate::common::err;

pub struct DatasetSummarizeTool {
    pub store: Arc<DatasetStore>,
}

#[derive(Debug, Deserialize, Serialize, agentik_proc::ToolInput)]
#[tool(
    name = "dataset_summarize",
    description = "Compute descriptive statistics for a numeric column in a registered dataset: count, mean, std_dev, variance, min, max, median, Q1/Q3, skewness, kurtosis. Null values are dropped before computation. Returns the stats as JSON — does not modify the dataset."
)]
pub struct DatasetSummarizeInput {
    #[desc = "Name of the registered dataset"]
    pub name: String,
    #[desc = "Numeric column to summarize"]
    pub column: String,
}

#[async_trait]
impl ToolFunction for DatasetSummarizeTool {
    type Input = DatasetSummarizeInput;

    fn timeout_seconds(&self) -> u64 { 300 }

    async fn run(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let name = input.name.trim();
        let column = input.column.trim();
        if name.is_empty() || column.is_empty() {
            return Ok(ToolResult::error("'name' and 'column' are required"));
        }

        let ds = self.store.get(name).await.map_err(err)?;
        let total = ds.row_count();
        let vals = ds.extract_f64(column, NullPolicy::DropNulls).map_err(err)?;
        let n = vals.len();
        let nulls_dropped = total - n;

        let mean = stat_primitives::descriptive::mean(&vals).map_err(err)?;
        let variance = stat_primitives::descriptive::variance(&vals).map_err(err)?;
        let std_dev = stat_primitives::descriptive::std_dev(&vals).map_err(err)?;
        let min = stat_primitives::descriptive::min(&vals).map_err(err)?;
        let max = stat_primitives::descriptive::max(&vals).map_err(err)?;
        let median = stat_primitives::descriptive::median(&vals).map_err(err)?;
        let q1 = stat_primitives::descriptive::quantile(&vals, 0.25).map_err(err)?;
        let q3 = stat_primitives::descriptive::quantile(&vals, 0.75).map_err(err)?;
        let skewness = stat_primitives::descriptive::skewness(&vals).map_err(err)?;
        let kurtosis = stat_primitives::descriptive::kurtosis(&vals).map_err(err)?;

        Ok(ToolResult::success_json(serde_json::json!({
            "dataset": name,
            "column": column,
            "n": n,
            "nulls_dropped": nulls_dropped,
            "mean": mean,
            "std_dev": std_dev,
            "variance": variance,
            "min": min,
            "max": max,
            "median": median,
            "q1": q1,
            "q3": q3,
            "skewness": skewness,
            "kurtosis": kurtosis,
        })))
    }
}
