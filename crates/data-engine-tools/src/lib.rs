mod add_edge_tool;
mod add_sink_tool;
mod add_source_tool;
mod add_sql_tool;
mod run_dag_tool;

use std::sync::Arc;

use agentik_core::tools::ToolRegistration;
use data_engine::runtime::DataEngineClient;

/// Build the default set of data-engine DAG tools.
///
/// Each tool sends commands to the [`DataEngineClient`] actor and awaits
/// replies via oneshot channels.
pub fn registrations(client: Arc<DataEngineClient>) -> Vec<ToolRegistration> {
    vec![
        ToolRegistration::from(add_source_tool::AddSourceNodeTool::new(client.clone())),
        ToolRegistration::from(add_sql_tool::AddSqlNodeTool::new(client.clone())),
        ToolRegistration::from(add_sink_tool::AddSinkNodeTool::new(client.clone())),
        ToolRegistration::from(add_edge_tool::AddEdgeTool::new(client.clone())),
        ToolRegistration::from(run_dag_tool::RunDagTool::new(client)),
    ]
}
