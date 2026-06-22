//! PubMed article detail tool: fetch full article record by PMID.

use agentik_core::tools::{ToolError, ToolFunction};
use agentik_sdk::types::ToolResult;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::common::{call_bridge, err};

#[derive(Debug, Deserialize, Serialize, agentik_proc::ToolInput)]
#[tool(
    name = "pubmed_article_detail",
    description = "Fetch the full record of a PubMed article by its PMID. Returns title, authors with affiliations, abstract, keywords, MeSH terms, references, similar articles, publication types, journal info, and full-text sources."
)]
pub struct ArticleDetailInput {
    #[desc = "PubMed ID of the article (e.g. '33423454')"]
    pub pmid: String,
}

pub struct ArticleDetailTool;

#[async_trait]
impl ToolFunction for ArticleDetailTool {
    type Input = ArticleDetailInput;

    fn timeout_seconds(&self) -> u64 {
        120
    }

    async fn run(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let payload = serde_json::json!({
            "action": "detail",
            "params": {
                "pmid": input.pmid,
            }
        });

        let response = call_bridge(&payload).await.map_err(err)?;

        match (response.ok, response.data, response.error) {
            (true, Some(data), _) => Ok(ToolResult::success_json(data)),
            (false, _, Some(error)) => Ok(ToolResult::error(error)),
            _ => Ok(ToolResult::error("unexpected bridge response".to_string())),
        }
    }
}
