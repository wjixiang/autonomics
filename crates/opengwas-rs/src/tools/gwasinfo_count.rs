use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction, ToolResult};
use agentik_sdk::types::ToolResult as AgentToolResult;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::json_err;
use crate::format::format_gwasinfo_count;
use crate::OpengwasClient;

#[derive(Debug, Deserialize, Serialize, agentik_proc::ToolInput)]
#[tool(
    name = "opengwas_gwasinfo_count",
    description = "Return the total number of GWAS datasets cached in memory."
)]
pub struct GwasinfoCountInput {}

pub struct GwasinfoCountTool {
    pub(crate) client: Arc<OpengwasClient>,
}

#[async_trait]
impl ToolFunction for GwasinfoCountTool {
    type Input = GwasinfoCountInput;

    async fn run(&self, _input: Self::Input) -> Result<AgentToolResult, ToolError> {
        let count = self.client.gwasinfo_count().await.map_err(json_err)?;
        Ok(AgentToolResult::success(format_gwasinfo_count(count)))
    }
}
