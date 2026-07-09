use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolFunction};
use agentik_proc::tool;
use agentik_sdk::types::ToolResult;
use async_trait::async_trait;
use data_engine::runtime::DataEngineClient;

use crate::ExecError;

#[tool(
    name = "add_linear_regression_node",
    description = "Add an OLS linear regression transform node to the DAG. \
                  Regresses the dependent `y_column` on one or more independent \
                  `x_columns` from the upstream input DataFrame, and outputs a \
                  summary DataFrame with coefficients, standard errors, \
                  t-statistics, p-values, R-squared, and the observation count."
)]
pub struct AddLinearRegressionNodeInput {
    #[desc = "Unique identifier for this node in the DAG"]
    pub id: String,
    #[desc = "Independent variable column names (one or more)"]
    pub x_columns: Vec<String>,
    #[desc = "Dependent variable column name"]
    pub y_column: String,
    #[desc = "Whether to include an intercept term. Defaults to true."]
    pub intercept: Option<bool>,
    #[desc = "Name for the output DataFrame. Defaults to the node id if omitted."]
    pub output_df_name: Option<String>,
}

pub struct AddLinearRegressionNodeTool {
    client: Arc<DataEngineClient>,
}

impl AddLinearRegressionNodeTool {
    pub fn new(client: Arc<DataEngineClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl ToolFunction for AddLinearRegressionNodeTool {
    type Input = AddLinearRegressionNodeInput;

    async fn run(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        if input.x_columns.is_empty() {
            return Ok(ToolResult::error(
                "x_columns must contain at least one column name",
            ));
        }
        let output_df_name = input.output_df_name.unwrap_or_else(|| input.id.clone());
        let intercept = input.intercept.unwrap_or(true);
        self.client
            .add_linear_regression_node(
                input.id,
                input.x_columns,
                input.y_column,
                intercept,
                output_df_name,
            )
            .await
            .map_err(ExecError::from)?;

        Ok(ToolResult::success("Linear regression node added to DAG"))
    }
}
