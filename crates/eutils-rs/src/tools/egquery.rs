use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction, ToolResult};
use agentik_sdk::types::ToolResult as AgentToolResult;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::EutilsClient;

#[derive(Debug, Deserialize, Serialize, agentik_proc::ToolInput)]
#[tool(
    name = "egquery",
    description = "Search all NCBI Entrez databases simultaneously with a text query. \
                  Returns the count of matching records for each database. Useful for \
                  discovering which databases contain relevant data for a given topic."
)]
pub struct EGQueryInput {
    #[desc = "Text query to search across all Entrez databases."]
    pub term: String,
}

pub struct EGQueryTool {
    pub(crate) client: Arc<EutilsClient>,
}

#[async_trait]
impl ToolFunction for EGQueryTool {
    type Input = EGQueryInput;

    fn timeout_seconds(&self) -> u64 {
        60
    }

    async fn run(&self, input: Self::Input) -> Result<AgentToolResult, ToolError> {
        let result = self
            .client
            .egquery(&input.term)
            .await
            .map_err(super::json_err)?;

        Ok(AgentToolResult::success_json(serde_json::to_value(result)?))
    }
}
