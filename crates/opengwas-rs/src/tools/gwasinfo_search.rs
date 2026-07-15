use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction, ToolResult};
use agentik_sdk::types::ToolResult as AgentToolResult;
use async_trait::async_trait;

use super::json_err;
use crate::OpengwasClient;
use crate::format::format_gwasinfo_table;
use agentik_proc::tool;

#[tool(
    name = "opengwas_gwasinfo_search",
    description = "Search cached GWAS datasets by keyword. Uses SQL LIKE \
                  matching on an indexed column. Results can be sorted by \
                  numeric fields (e.g. nsnp, sample_size, year) to quickly \
                  find the most relevant datasets."
)]
pub struct GwasinfoSearchInput {
    #[desc = "Search keyword (case-insensitive, matched as substring)."]
    pub keyword: String,
    #[desc = "Column to search on: 'trait' (phenotype name), 'author', or 'population'. Defaults to 'trait'."]
    pub field: Option<String>,
    #[desc = "Maximum number of results to return. Defaults to 50."]
    pub limit: Option<i64>,
    #[desc = "Column to sort results by: 'nsnp', 'sample_size', 'year', 'ncase', 'ncontrol', 'pmid', 'mr', 'priority', 'sd', 'author', 'trait'. Defaults to no sorting."]
    pub sort_by: Option<String>,
    #[desc = "Sort order: 'desc' (descending, default) or 'asc' (ascending)."]
    pub sort_order: Option<String>,
}

pub struct GwasinfoSearchTool {
    pub(crate) client: Arc<OpengwasClient>,
}

#[async_trait]
impl ToolFunction for GwasinfoSearchTool {
    type Input = GwasinfoSearchInput;

    async fn run(&self, input: Self::Input) -> Result<AgentToolResult, ToolError> {
        let field = input.field.unwrap_or_else(|| "trait".to_string());
        // Map external name "trait" → internal column name "trait_".
        let db_field = if field == "trait" { "trait_" } else { &field };
        let limit = input.limit.unwrap_or(50);

        // Normalize sort_by: map "trait" → "trait_" for the DB column.
        let sort_by = input
            .sort_by
            .as_deref()
            .map(|col| if col == "trait" { "trait_" } else { col });

        let result = self
            .client
            .gwasinfo_search(
                &input.keyword,
                db_field,
                limit,
                sort_by,
                input.sort_order.as_deref(),
            )
            .await
            .map_err(json_err)?;
        Ok(AgentToolResult::success(format_gwasinfo_table(
            &result,
            Some(&input.keyword),
            Some(&field),
        )))
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;
//
//     #[test]
//     fn test_search_by_trait() {
//
//     }
// }
