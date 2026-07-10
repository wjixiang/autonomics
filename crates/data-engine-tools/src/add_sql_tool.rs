use std::sync::Arc;

use agentik_sdk::types::ToolResult;
use async_trait::async_trait;
use data_engine::runtime::DataEngineClient;

use agentik_core::tools::{ToolError, ToolFunction};
use agentik_proc::tool;

use crate::ExecError;

#[tool(
    name = "add_sql_node",
    description = "Add a SQL transform node to the DAG. \
                  \
                  CRITICAL — how to reference upstream data in your SQL query: \
                  Each upstream input is registered as a temporary table in the shared \
                  SessionContext. The table name follows the pattern `{THIS_NODE_ID}__{INPUT_PORT_NAME}` \
                  (i.e. this node's own id, double underscore, then the input port name). \
                  \
                  - For a node with a single default input port (the common case), \
                    the table name is `{THIS_NODE_ID}__default`. \
                  - For multi-input nodes joined via add_edge with explicit to_port names \
                    (e.g. 'left', 'right'), use `{THIS_NODE_ID}__left` and `{THIS_NODE_ID}__right`. \
                  \
                  Do NOT use the upstream node's id as the table name — it will not work. \
                  \
                  Example: if this node's id is 'agg' and it receives data from a source \
                  node, write `SELECT * FROM agg__default`. If this node's id is 'join' with \
                  input ports 'left' and 'right', write `SELECT * FROM join__left JOIN join__right ON ...`."
)]
pub struct AddSqlNodeInput {
    #[desc = "Unique identifier for this node in the DAG. This id is also used to form the SQL table name (e.g. if id='agg', reference data as agg__default in your query)."]
    pub id: String,
    #[desc = "SQL query. Upstream data is accessible as tables named {this_node_id}__{port_name}. For single-input nodes: FROM {id}__default. For multi-input: FROM {id}__left, FROM {id}__right, etc."]
    pub query: String,
    #[desc = "Name for the output DataFrame / output port. Defaults to the node id if omitted. Downstream nodes will reference this name via their own table naming."]
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
