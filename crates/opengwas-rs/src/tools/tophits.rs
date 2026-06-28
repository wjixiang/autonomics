use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction, ToolResult};
use agentik_sdk::types::ToolResult as AgentToolResult;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::json_err;
use crate::format::format_tophits;
use crate::{OpengwasClient, types::*};

#[derive(Debug, Deserialize, Serialize, agentik_proc::ToolInput)]
#[tool(
    name = "opengwas_tophits",
    description = "Extract top hits from a GWAS dataset based on a p-value \
                  threshold. Supports optional clumping."
)]
pub struct TophitsInput {
    #[desc = "List of GWAS study IDs, e.g. ['ukb-b-19953']."]
    pub id: Vec<String>,
    #[desc = "P-value threshold (must be ≤ 0.01). Default 5e-8."]
    pub pval: Option<f64>,
    #[desc = "Whether to clump results: 1 (yes) or 0 (no). Default 1."]
    pub clump: Option<i32>,
    #[desc = "Clumping r2 threshold. Default 0.001."]
    pub r2: Option<f64>,
    #[desc = "Clumping window size in kb. Default 5000."]
    pub kb: Option<i32>,
    #[desc = "Reference population for clumping (EUR, SAS, EAS, AFR, AMR). Default EUR."]
    pub pop: Option<String>,
}

pub struct TophitsTool {
    pub(crate) client: Arc<OpengwasClient>,
}

#[async_trait]
impl ToolFunction for TophitsTool {
    type Input = TophitsInput;

    fn timeout_seconds(&self) -> u64 { 120 }

    async fn run(&self, input: Self::Input) -> Result<AgentToolResult, ToolError> {
        let result = self
            .client
            .tophits(&TophitsRequest {
                id: input.id,
                pval: input.pval,
                preclumped: None,
                clump: input.clump,
                r2: input.r2,
                kb: input.kb,
                pop: input.pop,
                commercial_approval_received: None,
            })
            .await
            .map_err(json_err)?;
        Ok(AgentToolResult::success(format_tophits(&result)))
    }
}
