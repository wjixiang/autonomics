use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction, ToolResult};
use agentik_sdk::types::ToolResult as AgentToolResult;
use async_trait::async_trait;

use super::json_err;
use crate::format::format_gwasinfo_table;
use crate::{OpengwasClient, types::*};
use agentik_proc::tool;

#[tool(
    name = "opengwas_gwasinfo",
    description = "Get metadata for specific GWAS datasets by their IDs \
                  (e.g. 'ieu-a-2', 'ukb-b-19953'). Results are served from \
                  the in-memory cache."
)]
pub struct GwasinfoByIdInput {
    #[desc = "List of GWAS study IDs to look up (e.g. ['ieu-a-2', 'ukb-b-19953'])."]
    pub id: Vec<String>,
}

pub struct GwasinfoByIdTool {
    pub(crate) client: Arc<OpengwasClient>,
}

#[async_trait]
impl ToolFunction for GwasinfoByIdTool {
    type Input = GwasinfoByIdInput;

    async fn run(&self, input: Self::Input) -> Result<AgentToolResult, ToolError> {
        let result = self
            .client
            .gwasinfo(&GwasInfoRequest { id: input.id })
            .await
            .map_err(json_err)?;
        Ok(AgentToolResult::success(format_gwasinfo_table(&result, None, None)))
    }
}
