use std::sync::Arc;

use crate::{EutilsClient, format::format_efetch};
use agentik_core::tools::{ToolError, ToolFunction, ToolResult};
use agentik_proc::tool;
use agentik_sdk::types::ToolResult as AgentToolResult;
use async_trait::async_trait;

#[tool(
    name = "pubmed_fetch",
    description = "Fetch PubMed articles by PMID(s). Returns article records in the \
                  requested format (abstract, MEDLINE, or full text). For structured \
                  summaries, use pubmed_summary instead."
)]
pub struct PubmedFetchInput {
    #[desc = "PubMed ID(s) as a comma-separated string (e.g. '33423454,30242208')."]
    pub pmid: String,
    #[desc = "Retrieval type: 'abstract' (default), 'medline', 'full', 'xml'."]
    pub rettype: Option<String>,
    #[desc = "Retrieval mode: 'text' (default) or 'xml'."]
    pub retmode: Option<String>,
    #[desc = "Maximum number of records to return (default 20, max 10000)."]
    pub retmax: Option<u32>,
}

pub struct PubmedFetchTool {
    pub(crate) client: Arc<EutilsClient>,
}

#[async_trait]
impl ToolFunction for PubmedFetchTool {
    type Input = PubmedFetchInput;

    fn timeout_seconds(&self) -> u64 {
        120
    }

    async fn run(&self, input: Self::Input) -> Result<AgentToolResult, ToolError> {
        let rettype = input.rettype.as_deref().unwrap_or("abstract");
        let retmode = input.retmode.as_deref().unwrap_or("text");

        let req = crate::types::EFetchRequest {
            db: "pubmed".into(),
            id: input.pmid,
            rettype: Some(rettype.to_owned()),
            retmode: Some(retmode.to_owned()),
            retmax: input.retmax,
            retstart: None,
            web_env: None,
            query_key: None,
        };

        let text = self.client.efetch(&req).await.map_err(super::json_err)?;

        let mut map = serde_json::Map::new();
        map.insert(
            "format".into(),
            serde_json::Value::String(rettype.to_owned()),
        );
        map.insert("content".into(), serde_json::Value::String(text));
        Ok(AgentToolResult::success(format_efetch(
            &serde_json::Value::Object(map),
        )))
    }
}
