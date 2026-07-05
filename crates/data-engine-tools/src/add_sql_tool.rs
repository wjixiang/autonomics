use std::sync::Arc;

use agentik_sdk::types::ToolResult;
use async_trait::async_trait;
use data_engine::runtime::DataEngineClient;

use agentik_core::tools::{ToolError, ToolFunction};

use agentik_proc::tool;

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
        self.client
            .add_sql_node(input.id, input.query)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        Ok(ToolResult::success("SQL node added to DAG"))
    }
}
