use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction, ToolResult};
use agentik_sdk::types::ToolResult as AgentToolResult;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::EutilsClient;

#[derive(Debug, Deserialize, Serialize, agentik_proc::ToolInput)]
#[tool(
    name = "pubmed_summary",
    description = "Retrieve document summaries for PubMed articles by PMID(s). \
                  Returns structured data including title, authors, journal, \
                  publication date, abstract, and DOI. Uses ESummary v2.0 for richer output."
)]
pub struct PubmedSummaryInput {
    #[desc = "PubMed ID(s) as a comma-separated string (e.g. '33423454,30242208')."]
    pub pmid: String,
    #[desc = "Maximum number of records to return (default 20)."]
    pub retmax: Option<u32>,
}

pub struct PubmedSummaryTool {
    pub(crate) client: Arc<EutilsClient>,
}

#[async_trait]
impl ToolFunction for PubmedSummaryTool {
    type Input = PubmedSummaryInput;

    fn timeout_seconds(&self) -> u64 {
        120
    }

    async fn run(&self, input: Self::Input) -> Result<AgentToolResult, ToolError> {
        let req = crate::types::ESummaryRequest {
            db: "pubmed".into(),
            id: input.pmid,
            retmax: input.retmax,
            retstart: None,
            version: Some("2.0".into()),
        };

        let result = self
            .client
            .esummary(&req)
            .await
            .map_err(super::json_err)?;

        Ok(AgentToolResult::success_json(result))
    }
}
