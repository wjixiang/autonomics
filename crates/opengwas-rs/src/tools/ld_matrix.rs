use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction, ToolResult};
use agentik_proc::tool;
use agentik_sdk::types::ToolResult as AgentToolResult;
use async_trait::async_trait;

use super::json_err;
use crate::format::format_ld_matrix;
use crate::{OpengwasClient, types::*};

#[tool(
    name = "opengwas_ld_matrix",
    description = "Get the LD R-value matrix for a list of SNPs. \
                  Values are relative to a specified reference allele."
)]
pub struct LdMatrixInput {
    #[desc = "List of rs IDs for the LD matrix."]
    pub rsid: Option<Vec<String>>,
    #[desc = "Reference population (EUR, SAS, EAS, AFR, AMR). Default EUR."]
    pub pop: Option<String>,
}

pub struct LdMatrixTool {
    pub(crate) client: Arc<OpengwasClient>,
}

#[async_trait]
impl ToolFunction for LdMatrixTool {
    type Input = LdMatrixInput;

    fn timeout_seconds(&self) -> u64 {
        120
    }

    async fn run(&self, input: Self::Input) -> Result<AgentToolResult, ToolError> {
        let result = self
            .client
            .ld_matrix(&LdMatrixRequest {
                rsid: input.rsid.unwrap_or_default(),
                pop: input.pop,
            })
            .await
            .map_err(json_err)?;
        Ok(AgentToolResult::success(format_ld_matrix(&result)))
    }
}
