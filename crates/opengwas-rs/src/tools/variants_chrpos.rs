use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction, ToolResult};
use agentik_sdk::types::ToolResult as AgentToolResult;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::json_err;
use crate::format::format_variants;
use crate::{OpengwasClient, types::*};

#[derive(Debug, Deserialize, Serialize, agentik_proc::ToolInput)]
#[tool(
    name = "opengwas_variants_chrpos",
    description = "Obtain variant information by chromosome:position format \
                  (hg19/b37). Optionally specify a search radius."
)]
pub struct VariantsChrposInput {
    #[desc = "List of chr:pos strings, e.g. ['7:105561135', '10:44865737']."]
    pub chrpos: Option<Vec<String>>,
    #[desc = "Range in bp to search either side of each locus. Default 0."]
    pub radius: Option<i32>,
}

pub struct VariantsChrposTool {
    pub(crate) client: Arc<OpengwasClient>,
}

#[async_trait]
impl ToolFunction for VariantsChrposTool {
    type Input = VariantsChrposInput;

    async fn run(&self, input: Self::Input) -> Result<AgentToolResult, ToolError> {
        let result = self
            .client
            .variants_chrpos(&VariantsChrposRequest {
                chrpos: input.chrpos.unwrap_or_default(),
                radius: input.radius,
            })
            .await
            .map_err(json_err)?;
        Ok(AgentToolResult::success(format_variants(&result)))
    }
}
