use std::sync::Arc;

use agentik_sdk::types::ToolResult;
use async_trait::async_trait;
use data_engine::runtime::DataEngineClient;

use agentik_core::tools::{ToolError, ToolFunction};
use agentik_proc::tool;

use crate::ExecError;

#[tool(
    name = "add_sql_node",
    description = "Add a SQL transform node to the DAG: runs a SQL query over the upstream inputs. \
                  \
                  TABLES — how to reference upstream data: \
                  Each upstream input is registered as a temp table `port_{N}`, where N is the \
                  input port index (0-based). Single-input node (the common case) -> `port_0`. \
                  Multi-input (e.g. a 2-input join) -> `port_0` and `port_1`, in the order edges \
                  were added. Use ONLY `port_N` — never the upstream node id or any other name. \
                  \
                  COLUMNS — case sensitivity (the #1 cause of 'No field named ...' failures): \
                  DataFusion is case-sensitive and lowercases UNQUOTED identifiers. Column names \
                  from CSV/parquet keep their original casing (e.g. `Species`, `PetalLengthCm`, \
                  `Id`), so an unquoted `species` or `petallengthcm` will NOT resolve. ALWAYS wrap \
                  column names in double quotes using their exact case: `\"Species\"`, \
                  `\"PetalLengthCm\"`. Table names `port_0` / `port_1` are already lowercase and \
                  need no quoting. Read the exact column names and casing from the upstream node's \
                  reported `output_schema` BEFORE writing the SQL. \
                  \
                  Examples: \
                  - Filter:     SELECT * FROM port_0 WHERE \"Species\" = 'Iris-setosa' \
                  - Aggregate:  SELECT \"Species\", COUNT(*) AS n FROM port_0 GROUP BY \"Species\" \
                  - 2-input join: SELECT * FROM port_0 JOIN port_1 ON port_0.\"Id\" = port_1.\"Id\""
)]
pub struct AddSqlNodeInput {
    #[desc = "Unique identifier for this node in the DAG."]
    pub id: String,
    #[desc = "SQL query. Reference upstream inputs as tables `port_0`, `port_1`, ... (never the node id). \
              Column names are case-sensitive: ALWAYS double-quote them in exact case (e.g. `\"Species\"`, \
              `\"PetalLengthCm\"`) as reported by the upstream node's output_schema — unquoted identifiers \
              are lowercased and will not match mixed-case columns."]
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
            .map_err(ExecError::from)?;

        Ok(ToolResult::success("SQL node added to DAG"))
    }
}
