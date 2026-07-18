use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction};
use agentik_proc::tool;
use agentik_sdk::types::ToolResult;
use async_trait::async_trait;
use data_engine::runtime::DataEngineClient;

use crate::ExecError;

#[tool(
    name = "add_node",
    description = "Add a node to the DAG by its registered kind and a JSON spec. \
                  \
                  WORKFLOW — discover then create: \
                  1. Use `list_node_factories` to see available node kinds and their \
                     JSON Schemas. \
                  2. (Optional) Use `get_node_spec` to inspect the full JSON Schema \
                     for a specific kind. \
                  3. Pass the node `id`, `kind`, and a `spec` object conforming to \
                     the schema into this tool. \
                  \
                  Each node kind expects different spec fields — match the JSON \
                  Schema exactly. Common examples: \
                  - \"sql\":            {\"sql_query\": \"SELECT * FROM port_0\"} \
                  - \"source\":         {\"type\": \"file\", \"path\": \"/data/sample.vcf.gz\", \"format\": null} \
                  - \"source\":         {\"type\": \"iceberg\", \"ident\": \"gwas.study\"} \
                  - \"sink\":           {\"type\": \"file\", \"path\": \"/out/result.csv\", \"format\": \"csv\", \"mode\": \"overwrite\"} \
                  - \"linear_regression\": {\"x_columns\": [\"x1\"], \"y_column\": \"y\", \"intercept\": true} \
                  - \"ldsc\":           {\"m\": [1000000.0], \"n_blocks\": 200, \"intercept\": null} \
                  - \"mock\":           {}"
)]
pub struct AddNodeInput {
    /// Unique identifier for this node in the DAG.
    pub id: String,
    /// The node kind — one of the kinds returned by `list_node_factories`
    /// (e.g. "sql", "source", "sink", "linear_regression", "ldsc", "mock").
    pub kind: String,
    /// JSON object conforming to the node's JSON Schema. Can include extra
    /// fields — the node factory ignores unknown keys.
    pub spec: serde_json::Value,
}

pub struct AddNodeTool {
    client: Arc<DataEngineClient>,
}

impl AddNodeTool {
    pub fn new(client: Arc<DataEngineClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl ToolFunction for AddNodeTool {
    type Input = AddNodeInput;

    async fn run(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        self.client
            .add_node(input.id, input.kind, input.spec)
            .await
            .map_err(ExecError::from)?;

        Ok(ToolResult::success("node added to DAG"))
    }
}
