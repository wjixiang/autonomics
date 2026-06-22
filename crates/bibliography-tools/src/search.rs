//! PubMed search tool: search articles by keyword.

use agentik_core::tools::{ToolError, ToolFunction};
use agentik_sdk::types::ToolResult;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::common::{call_bridge, err};

#[derive(Debug, Deserialize, Serialize, agentik_proc::ToolInput)]
#[tool(
    name = "pubmed_search",
    description = "Search PubMed for articles by keyword. Returns a list of article profiles with PMID, title, authors, journal citation, and a snippet. Supports sorting and date/publication-type filters."
)]
pub struct PubmedSearchInput {
    #[desc = "Free-text search query for PubMed"]
    pub term: String,
    #[desc = "Sort order: 'match' (relevance), 'date', 'pubdate', 'fauth' (first author), 'jour' (journal). Defaults to 'match'."]
    pub sort: Option<String>,
    #[desc = "Page number (1-indexed). Defaults to 1."]
    pub page: Option<u32>,
    #[desc = "Filters: date ranges (e.g. '2020:2024'), publication types (e.g. 'clinical trial', 'review', 'systematic review', 'meta-analysis', 'randomized controlled trial')."]
    pub filter: Option<Vec<String>>,
}

pub struct PubmedSearchTool;

#[async_trait]
impl ToolFunction for PubmedSearchTool {
    type Input = PubmedSearchInput;

    fn timeout_seconds(&self) -> u64 {
        120
    }

    async fn run(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let payload = serde_json::json!({
            "action": "search",
            "params": {
                "term": input.term,
                "sort": input.sort.unwrap_or_else(|| "match".to_string()),
                "page": input.page,
                "filter": input.filter.unwrap_or_default(),
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
