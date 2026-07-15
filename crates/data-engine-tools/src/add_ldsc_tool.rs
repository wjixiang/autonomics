use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction};
use agentik_proc::tool;
use agentik_sdk::types::ToolResult;
use async_trait::async_trait;
use data_engine::LdscHsqConfig;
use data_engine::runtime::DataEngineClient;
use datalake::Datalake;

use crate::ExecError;

#[tool(
    name = "add_ldsc_node",
    description = "Add an LD Score Regression (LDSC) node to the DAG for estimating \
                  SNP-heritability (h2) and the LD Score regression intercept. \
                  The node accepts raw GWAS summary statistics as input (containing \
                  Z-scores, sample sizes, and rsid) and internally queries the Iceberg \
                  data lake (genetics.ld_score) for LD Score panel data, joining on rsid \
                  before running LDSC. \
                  Outputs a single-row summary DataFrame with h2, h2_se, intercept, \
                  intercept_se, ratio, ratio_se, mean_chisq, lambda_gc, n_snp, coef (JSON), \
                  and coef_se (JSON)."
)]
pub struct AddLdscNodeInput {
    #[desc = "Unique identifier for this node in the DAG"]
    pub id: String,
    #[desc = "Name of the Z-score column in the input sumstats DataFrame"]
    pub z_column: String,
    #[desc = "Name of the per-SNP sample size column in the input sumstats DataFrame"]
    pub n_column: String,
    #[desc = "Name of the rsid (SNP identifier) column used for the join. Defaults to 'rsid'."]
    pub rsid_column: Option<String>,
    // #[desc = "LD Score panel table name under the genetics.ld_score namespace (e.g. 'panel'). Defaults to 'panel'."]
    // pub ld_score_table: Option<String>,
    #[desc = "Per-annotation L2-summed M values (one per annotation; baseline LDSC uses a single value)"]
    pub m: Vec<f64>,
    #[desc = "Number of block-jackknife blocks for standard error estimation. Defaults to 200."]
    pub n_blocks: Option<usize>,
    #[desc = "Fixed intercept value, or omit for a free intercept (the standard case)"]
    pub intercept: Option<f64>,
}

pub struct AddLdscNodeTool {
    client: Arc<DataEngineClient>,
    datalake: Arc<Datalake>,
}

impl AddLdscNodeTool {
    pub fn new(client: Arc<DataEngineClient>, datalake: Arc<Datalake>) -> Self {
        Self { client, datalake }
    }
}

#[async_trait]
impl ToolFunction for AddLdscNodeTool {
    type Input = AddLdscNodeInput;

    async fn run(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        if input.m.is_empty() {
            return Ok(ToolResult::error("m must contain at least one value"));
        }
        let n_blocks = input.n_blocks.unwrap_or(200);
        let rsid_column = input.rsid_column.unwrap_or_else(|| "rsid".to_string());
        let ldsc = LdscHsqConfig::new(input.m, n_blocks, input.intercept);
        self.client
            .add_ldsc_node(
                input.id,
                self.datalake.clone(),
                input.z_column,
                input.n_column,
                rsid_column,
                ldsc,
            )
            .await
            .map_err(ExecError::from)?;

        Ok(ToolResult::success("LDSC node added to DAG"))
    }
}
