//! L4 analysis tool: inverse-variance weighted (IVW) meta-analysis.
//!
//! Two-sample Mendelian randomization (MR) tool. Extracts beta_exposure,
//! beta_outcome, and se_outcome columns, then calls
//! [`stat_primitives::meta::ivw`].

use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction};
use agentik_sdk::types::ToolResult;
use async_trait::async_trait;
use data_engine::{DatasetStore, NullPolicy};
use serde::{Deserialize, Serialize};

use crate::common::err;

pub struct DatasetIvwTool {
    pub store: Arc<DatasetStore>,
}

#[derive(Debug, Deserialize, Serialize, agentik_proc::ToolInput)]
#[tool(
    name = "dataset_ivw",
    description = "Run inverse-variance weighted (IVW) meta-analysis on a registered dataset. Provide columns for the exposure SNP (beta_exposure), outcome SNP (beta_outcome), and standard error (se_outcome). This is a two-sample Mendelian randomization method. The result is stored as a new dataset (default name: 'ivw')."
)]
pub struct DatasetIvwInput {
    #[desc = "Name of the registered dataset"]
    pub name: String,
    #[desc = "Column containing SNP-exposure effect sizes (beta_X)"]
    pub beta_exposure: String,
    #[desc = "Column containing SNP-outcome effect sizes (beta_Y)"]
    pub beta_outcome: String,
    #[desc = "Column containing standard errors for the outcome"]
    pub se_outcome: String,
    #[desc = "Use random-effects model. Defaults to true."]
    pub random_effects: Option<bool>,
    #[desc = "Output dataset name. Defaults to 'ivw'."]
    pub output: Option<String>,
}

#[async_trait]
impl ToolFunction for DatasetIvwTool {
    type Input = DatasetIvwInput;

    fn timeout_seconds(&self) -> u64 { 300 }

    async fn run(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let name = input.name.trim();
        let beta_exp = input.beta_exposure.trim();
        let beta_out = input.beta_outcome.trim();
        let se_out = input.se_outcome.trim();
        if name.is_empty() || beta_exp.is_empty() || beta_out.is_empty() || se_out.is_empty() {
            return Ok(ToolResult::error(
                "'name', 'beta_exposure', 'beta_outcome', and 'se_outcome' are all required",
            ));
        }
        let random_effects = input.random_effects.unwrap_or(true);
        let output_name = input.output.as_deref().map(str::trim).unwrap_or("ivw");

        let ds = self.store.get(name).await.map_err(err)?;
        let beta_exp_vals = ds.extract_f64(beta_exp, &NullPolicy::DropNulls).map_err(err)?;
        let beta_out_vals = ds.extract_f64(beta_out, &NullPolicy::DropNulls).map_err(err)?;
        let se_out_vals = ds.extract_f64(se_out, &NullPolicy::DropNulls).map_err(err)?;

        let n = beta_exp_vals.len();
        if beta_out_vals.len() != n || se_out_vals.len() != n {
            return Ok(ToolResult::error(
                "all three columns must have the same non-null row count",
            ));
        }

        let result = stat_primitives::meta::ivw(
            &beta_exp_vals,
            &beta_out_vals,
            &se_out_vals,
            random_effects,
        )
        .map_err(err)?;

        Ok(ToolResult::success_json(serde_json::json!({
            "dataset": output_name,
            "method": "IVW",
            "n_snps": result.n_snps,
            "estimate": result.estimate,
            "se": result.se,
            "p_value": result.p_value,
            "q_statistic": result.q_statistic,
            "q_p_value": result.q_p_value,
            "i_squared": result.i_squared,
            "tau_squared": result.tau_squared,
            "random_effects": random_effects,
        })))
    }
}
