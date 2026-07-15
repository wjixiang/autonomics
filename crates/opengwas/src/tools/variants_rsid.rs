use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction, ToolResult};
use agentik_sdk::types::ToolResult as AgentToolResult;
use async_trait::async_trait;

use super::json_err;
use crate::format::format_variants;
use crate::{OpengwasClient, types::*};
use agentik_proc::tool;

#[tool(
    name = "opengwas_variants_rsid",
    description = "Obtain variant information by rs IDs. Returns chromosome, \
                  position, alleles, and other annotation data."
)]
pub struct VariantsRsidInput {
    #[desc = "List of variant rs IDs, e.g. ['rs1205', 'rs234']."]
    pub rsid: Option<Vec<String>>,
}

pub struct VariantsRsidTool {
    pub(crate) client: Arc<OpengwasClient>,
}

#[async_trait]
impl ToolFunction for VariantsRsidTool {
    type Input = VariantsRsidInput;

    async fn run(&self, input: Self::Input) -> Result<AgentToolResult, ToolError> {
        let result = self
            .client
            .variants_rsid(&VariantsRsidRequest {
                rsid: input.rsid.unwrap_or_default(),
            })
            .await
            .map_err(json_err)?;
        Ok(AgentToolResult::success(format_variants(&result)))
    }
}
