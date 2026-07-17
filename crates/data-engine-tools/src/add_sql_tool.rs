use std::sync::Arc;

use agentik_sdk::types::ToolResult;
use async_trait::async_trait;
use data_engine::runtime::DataEngineClient;

use agentik_core::tools::{ToolError, ToolFunction};
use agentik_proc::tool;

use crate::ExecError;

// NOTE: DataFusion 53.x does NOT accept `UNNEST(...)` as a FROM-clause table factor —
// not the bare form, not `WITH ORDINALITY`, not `WITH OFFSET`; the planner returns
// `not_impl_err`. The scalar `unnest()` function works but explodes row counts and has
// no ordinality, so it is also off-limits. Both are forbidden by the tool prompt below
// (LIST-TYPED COLUMNS section), which directs the agent to use array functions instead.
// Tracked upstream at apache/datafusion#22310; lift the prompt constraint once we bump
// to a DataFusion release that includes ordinality / offset support.
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
                  LIST-TYPED COLUMNS — DO NOT use UNNEST: \
                  Some upstream sources (notably VCF via `read_vcf`) emit columns as `List<...>` \
                  (e.g. VCF `id`, `alt`, `filter` are `List<Utf8>`; FORMAT/sample columns are \
                  nested structs of per-sample arrays). DataFusion 53.x does NOT accept \
                  `UNNEST(...)` as a FROM-clause table factor — not the bare form, not \
                  `WITH ORDINALITY`, not `WITH OFFSET` — the planner returns `not_impl_err`. The \
                  scalar `unnest(col)` function works but explodes row counts and has no \
                  ordinality, so do not reach for it either. Treat List fields as opaque arrays \
                  and use array functions instead: \
                    - element access    → `array_element(col, n)` or `col[n]` (1-indexed) \
                    - length / empty    → `array_length(col)`, `array_empty(col)` \
                    - flatten to scalar → `array_to_string(col, ';')` \
                    - membership        → `array_has(col, 'x')`, `array_contains(col, 'x')` \
                  Inspect the upstream node's `output_schema` BEFORE writing the SQL — if a \
                  column is a List and you find yourself wanting to explode it into rows, you \
                  want an array function, not UNNEST. \
                  \
                  Examples: \
                  - Filter:     SELECT * FROM port_0 WHERE \"Species\" = 'Iris-setosa' \
                  - Aggregate:  SELECT \"Species\", COUNT(*) AS n FROM port_0 GROUP BY \"Species\" \
                  - 2-input join: SELECT * FROM port_0 JOIN port_1 ON port_0.\"Id\" = port_1.\"Id\" \
                  - List field: SELECT \"id\"[1] AS id_scalar, array_length(\"alt\") AS n_alts, \
                                 array_to_string(\"filter\", ';') AS filter_str \
                                 FROM port_0 WHERE array_has(\"filter\", 'PASS')"
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
