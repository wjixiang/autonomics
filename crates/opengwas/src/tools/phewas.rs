use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction, ToolResult};
use agentik_sdk::types::ToolResult as AgentToolResult;
use async_trait::async_trait;

use super::json_err;
use crate::format::format_phewas;
use crate::{OpengwasClient, types::*};
use agentik_proc::tool;

#[tool(
    name = "opengwas_phewas",
    description = "Perform PheWAS of specified variants across all available \
                  GWAS datasets. Only accepts p ≤ 0.01."
)]
pub struct PhewasInput {
    #[desc = "List of variant identifiers (rsID, chr:pos, or chr:pos range on hg19/b37)."]
    pub variant: Vec<String>,
    #[desc = "P-value threshold (must ≤ 0.01). Default 0.01."]
    pub pval: Option<f64>,
    #[desc = "Restrict search to specific study indexes. If empty, searches all."]
    pub index_list: Option<Vec<String>>,
}

pub struct PhewasTool {
    pub(crate) client: Arc<OpengwasClient>,
}

#[async_trait]
impl ToolFunction for PhewasTool {
    type Input = PhewasInput;

    fn timeout_seconds(&self) -> u64 {
        120
    }

    async fn run(&self, input: Self::Input) -> Result<AgentToolResult, ToolError> {
        let result = self
            .client
            .phewas(&PhewasRequest {
                variant: input.variant,
                pval: input.pval,
                index_list: input.index_list,
                commercial_approval_received: None,
            })
            .await
            .map_err(json_err)?;
        Ok(AgentToolResult::success(format_phewas(&result)))
    }
}
