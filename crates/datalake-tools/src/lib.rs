mod describe_table_tool;
mod query_iceberg_tool;

use std::sync::Arc;

use agentik_core::tools::ToolRegistration;
use datalake::Datalake;

pub use describe_table_tool::DescribeIcebergTableTool;
pub use query_iceberg_tool::QueryIcebergTool;

/// Build the set of Iceberg data-lake tools.
///
/// Each tool holds a shared [`Datalake`] handle and queries the catalog
/// directly (no DAG / data-engine required).
pub fn registrations(datalake: Arc<Datalake>) -> Vec<ToolRegistration> {
    vec![
        ToolRegistration::from(QueryIcebergTool::new(datalake.clone())),
        ToolRegistration::from(DescribeIcebergTableTool::new(datalake)),
    ]
}
