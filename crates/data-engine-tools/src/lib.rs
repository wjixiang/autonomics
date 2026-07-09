mod add_edge_tool;
mod add_linear_regression_tool;
mod add_sink_tool;
mod add_source_tool;
mod add_sql_tool;
mod get_output_tool;
mod remove_node_tool;
mod run_dag_tool;
mod view_dag_tool;
mod clear_dag_tool;

use std::sync::Arc;

use agentik_core::tools::{ToolError, ToolRegistration};
use data_engine::runtime::DataEngineClient;

/// Shared tool execution error — replaces `anyhow` with typed variants
/// so each error source is identifiable without string matching.
#[derive(Debug)]
pub(crate) enum ExecError {
    /// A parse/validation issue in tool input (e.g. unknown format string).
    Format(String),
    /// An error from the data engine actor.
    Client(data_engine::runtime::error::ClientError),
}

impl From<String> for ExecError {
    fn from(msg: String) -> Self {
        Self::Format(msg)
    }
}

impl From<data_engine::runtime::error::ClientError> for ExecError {
    fn from(e: data_engine::runtime::error::ClientError) -> Self {
        Self::Client(e)
    }
}

impl std::fmt::Display for ExecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Format(msg) => write!(f, "{msg}"),
            Self::Client(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ExecError {}

// Safety: ExecError only contains String and ClientError, both of which
// are Send + Sync.
unsafe impl Send for ExecError {}
unsafe impl Sync for ExecError {}

impl From<ExecError> for ToolError {
    fn from(e: ExecError) -> Self {
        ToolError::ExecutionFailed {
            source: Box::new(e),
        }
    }
}

/// Build the default set of data-engine DAG tools.
///
/// Each tool sends commands to the [`DataEngineClient`] actor and awaits
/// replies via oneshot channels.
pub fn registrations(client: Arc<DataEngineClient>) -> Vec<ToolRegistration> {
    vec![
        ToolRegistration::from(add_source_tool::AddSourceNodeTool::new(client.clone())),
        ToolRegistration::from(add_sql_tool::AddSqlNodeTool::new(client.clone())),
        ToolRegistration::from(add_sink_tool::AddSinkNodeTool::new(client.clone())),
        ToolRegistration::from(
            add_linear_regression_tool::AddLinearRegressionNodeTool::new(client.clone()),
        ),
        ToolRegistration::from(add_edge_tool::AddEdgeTool::new(client.clone())),
        ToolRegistration::from(run_dag_tool::RunDagTool::new(client.clone())),
        ToolRegistration::from(get_output_tool::GetOutputTool::new(client.clone())),
        ToolRegistration::from(remove_node_tool::RemoveNodeTool::new(client.clone())),
        ToolRegistration::from(view_dag_tool::ViewDagTool::new(client.clone())),
        ToolRegistration::from(clear_dag_tool::ClearDagTool::new(client)),
    ]
}
