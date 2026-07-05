use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction, ToolResult};
use agentik_sdk::types::ToolResult as AgentToolResult;
use async_trait::async_trait;
use crate::{EutilsClient, format::format_elink};
use agentik_proc::tool;

#[tool(
    name = "pubmed_related",
    description = "Find related articles in PubMed for one or more PMIDs. \
                  Uses NCBI ELink with neighbor_score to return related PMIDs \
                  with relevance scores. Also supports cross-database links \
                  (e.g. gene → pubmed)."
)]
pub struct PubmedRelatedInput {
    #[desc = "PubMed ID(s) as a comma-separated string to find related articles for."]
    pub pmid: String,
    #[desc = "Source database (default 'pubmed'). Use 'gene' to link genes to PubMed articles."]
    pub dbfrom: Option<String>,
    #[desc = "Target database (default 'pubmed')."]
    pub db: Option<String>,
    #[desc = "Specific link name to retrieve (e.g. 'pubmed_pubmed_related')."]
    pub linkname: Option<String>,
}

pub struct PubmedRelatedTool {
    pub(crate) client: Arc<EutilsClient>,
}

#[async_trait]
impl ToolFunction for PubmedRelatedTool {
    type Input = PubmedRelatedInput;

    fn timeout_seconds(&self) -> u64 {
        120
    }

    async fn run(&self, input: Self::Input) -> Result<AgentToolResult, ToolError> {
        let dbfrom = input.dbfrom.as_deref().unwrap_or("pubmed");

        let req = crate::types::ELinkRequest {
            dbfrom: dbfrom.to_owned(),
            id: input.pmid,
            db: input.db,
            cmd: Some("neighbor".into()),
            linkname: input.linkname,
        };

        let result = self
            .client
            .elink(&req)
            .await
            .map_err(super::json_err)?;

        Ok(AgentToolResult::success(format_elink(&result)))
    }
}
