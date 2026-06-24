use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction, ToolResult};
use agentik_sdk::types::ToolResult as AgentToolResult;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::EutilsClient;

#[derive(Debug, Deserialize, Serialize, agentik_proc::ToolInput)]
#[tool(
    name = "pubmed_spell",
    description = "Get spelling suggestions for a PubMed search term. \
                  Useful for correcting misspelled gene names, author names, \
                  or other biomedical terms before running a search."
)]
pub struct PubmedSpellInput {
    #[desc = "The search term to check spelling for."]
    pub term: String,
}

pub struct PubmedSpellTool {
    pub(crate) client: Arc<EutilsClient>,
}

#[async_trait]
impl ToolFunction for PubmedSpellTool {
    type Input = PubmedSpellInput;

    fn timeout_seconds(&self) -> u64 {
        60
    }

    async fn run(&self, input: Self::Input) -> Result<AgentToolResult, ToolError> {
        let result = self
            .client
            .espell("pubmed", &input.term)
            .await
            .map_err(super::json_err)?;

        Ok(AgentToolResult::success_json(result))
    }
}
