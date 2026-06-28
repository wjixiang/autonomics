use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction, ToolResult};
use agentik_sdk::types::ToolResult as AgentToolResult;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::json_err;
use crate::format::format_ld_clump;
use crate::{OpengwasClient, types::*};

#[derive(Debug, Deserialize, Serialize, agentik_proc::ToolInput)]
#[tool(
    name = "opengwas_ld_clump",
    description = "Perform LD clumping on a set of rs IDs using 1000 Genomes \
                  reference data. Returns independent loci after clumping."
)]
pub struct LdClumpInput {
    #[desc = "List of rs IDs to clump."]
    pub rsid: Option<Vec<String>>,
    #[desc = "P-values for each SNP (same length as rsid)."]
    pub pval: Option<Vec<f64>>,
    #[desc = "Significance threshold. Default 5e-8."]
    pub pthresh: Option<f64>,
    #[desc = "LD r2 threshold for clumping. Default 0.001."]
    pub r2: Option<f64>,
    #[desc = "Clumping window size in kb. Default 5000."]
    pub kb: Option<i32>,
    #[desc = "Reference population (EUR, SAS, EAS, AFR, AMR). Default EUR."]
    pub pop: Option<String>,
}

pub struct LdClumpTool {
    pub(crate) client: Arc<OpengwasClient>,
}

#[async_trait]
impl ToolFunction for LdClumpTool {
    type Input = LdClumpInput;

    fn timeout_seconds(&self) -> u64 { 120 }

    async fn run(&self, input: Self::Input) -> Result<AgentToolResult, ToolError> {
        let result = self
            .client
            .ld_clump(&LdClumpRequest {
                rsid: input.rsid.unwrap_or_default(),
                pval: input.pval.unwrap_or_default(),
                pthresh: input.pthresh,
                r2: input.r2,
                kb: input.kb,
                pop: input.pop,
            })
            .await
            .map_err(json_err)?;
        Ok(AgentToolResult::success(format_ld_clump(&result)))
    }
}
