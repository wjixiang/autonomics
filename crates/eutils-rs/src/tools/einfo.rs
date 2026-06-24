use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction, ToolResult};
use agentik_sdk::types::ToolResult as AgentToolResult;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::EutilsClient;

#[derive(Debug, Deserialize, Serialize, agentik_proc::ToolInput)]
#[tool(
    name = "einfo",
    description = "Get information about NCBI Entrez databases. If db is not specified, \
                  returns a list of all available databases. If db is specified, returns \
                  field counts, searchable fields, available links, and last update date \
                  for that database."
)]
pub struct EInfoInput {
    #[desc = "Database name to query (e.g. 'pubmed', 'gene', 'nucleotide'). If omitted, lists all databases."]
    pub db: Option<String>,
}

pub struct EInfoTool {
    pub(crate) client: Arc<EutilsClient>,
}

#[async_trait]
impl ToolFunction for EInfoTool {
    type Input = EInfoInput;

    fn timeout_seconds(&self) -> u64 {
        30
    }

    async fn run(&self, input: Self::Input) -> Result<AgentToolResult, ToolError> {
        let result = self
            .client
            .einfo(input.db.as_deref())
            .await
            .map_err(super::json_err)?;

        Ok(AgentToolResult::success_json(serde_json::to_value(result)?))
    }
}
