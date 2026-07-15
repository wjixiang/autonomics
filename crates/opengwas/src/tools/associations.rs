use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction, ToolResult};
use agentik_sdk::types::ToolResult as AgentToolResult;
use async_trait::async_trait;

use super::json_err;
use crate::format::format_associations;
use crate::{OpengwasClient, types::*};
use agentik_proc::tool;

#[tool(
    name = "opengwas_associations",
    description = "Get specific variant associations from specific GWAS datasets. \
                  Provide variant rsIDs (or chr:pos) and study IDs. Returns \
                  association data including beta, se, p-value, effect/non-effect alleles."
)]
pub struct AssociationsInput {
    #[desc = "List of variants as rsid or chr:pos (hg19/b37), e.g. ['rs1205', '7:105561135']."]
    pub variant: Vec<String>,
    #[desc = "List of GWAS study IDs, e.g. ['ieu-a-2', 'ukb-b-19953']."]
    pub id: Vec<String>,
    #[desc = "Whether to look for proxies: 1 (yes) or 0 (no). Default 0."]
    pub proxies: Option<i32>,
    #[desc = "Reference population for proxies (AFR, AMR, EAS, EUR, SAS). Default EUR."]
    pub population: Option<String>,
    #[desc = "Minimum LD r2 for a proxy. Default 0.8."]
    pub r2: Option<f64>,
}

pub struct AssociationsTool {
    pub(crate) client: Arc<OpengwasClient>,
}

#[async_trait]
impl ToolFunction for AssociationsTool {
    type Input = AssociationsInput;

    fn timeout_seconds(&self) -> u64 {
        120
    }

    async fn run(&self, input: Self::Input) -> Result<AgentToolResult, ToolError> {
        let result = self
            .client
            .associations(&AssociationsRequest {
                variant: input.variant,
                id: input.id,
                proxies: input.proxies,
                population: input.population,
                r2: input.r2,
                align_alleles: None,
                palindromes: None,
                maf_threshold: None,
                commercial_approval_received: None,
            })
            .await
            .map_err(json_err)?;
        Ok(AgentToolResult::success(format_associations(&result)))
    }
}
