use std::sync::Arc;

use agentik_sdk::types::ToolResult;
use async_trait::async_trait;
use data_engine::runtime::DataEngineClient;

use agentik_core::tools::{ToolError, ToolFunction};
use agentik_proc::tool;

use crate::ExecError;

#[tool(
    name = "add_sql_node",
    description = "Add a SQL transform node to the DAG. The query references \
                  upstream nodes by their port names (default port is 'src')."
)]
pub struct AddSqlNodeInput {
    #[desc = "Unique identifier for this node in the DAG"]
    pub id: String,
    #[desc = "SQL query to execute over upstream data"]
    pub query: String,
    #[desc = "Name for the output DataFrame. Defaults to the node id if omitted."]
    pub output_df_name: Option<String>,
}

pub struct AddSqlNodeTool {
    client: Arc<DataEngineClient>,
}

impl AddSqlNodeTool {
    pub fn new(client: Arc<DataEngineClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl ToolFunction for AddSqlNodeTool {
    type Input = AddSqlNodeInput;

    async fn run(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let output_df_name = input.output_df_name.unwrap_or_else(|| input.id.clone());
        self.client
            .add_sql_node(input.id, input.query, output_df_name)
            .await
            .map_err(ExecError::from)?;

        Ok(ToolResult::success("SQL node added to DAG"))
    }
}
