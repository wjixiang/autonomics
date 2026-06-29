//! Aether agent tools: Iceberg namespace/table CRUD (REST catalog) and
//! DataFusion-level table discovery, schema inspection, and SQL preview.
//!
//! All tools operate through the shared [`DataSession`] so that the
//! underlying REST catalog connection and DataFusion session are reused
//! across every invocation.
//!
//! Wire the tools into an agent's toolset via [`iceberg_registrations`].

pub mod common;
pub mod dataset;
pub mod describe_table;
pub mod list_tables;
pub mod namespace;
pub mod query;
pub mod table;

use std::sync::Arc;

use iceberg::Namespace;
use serde_json::{Value, json};

pub use agentik_core::tools::ToolRegistration;
pub use data_engine::data_session::DataSession;
pub use dataset::{
    DatasetDescribeInput, DatasetDescribeTool, DatasetDropInput, DatasetDropTool,
    DatasetEggerInput, DatasetEggerTool, DatasetIvwInput, DatasetIvwTool,
    DatasetLimitInput, DatasetLimitTool, DatasetListInput, DatasetListTool,
    DatasetLoadTableInput, DatasetLoadTableTool, DatasetMapInput, DatasetMapTool,
    DatasetOlsInput, DatasetOlsTool, DatasetPreviewInput, DatasetPreviewTool,
    DatasetSelectInput, DatasetSelectTool, DatasetSortInput, DatasetSortTool,
    DatasetSqlInput, DatasetSqlTool, DatasetSummarizeInput, DatasetSummarizeTool,
    DatasetUnionInput, DatasetUnionTool,
    dataset_registrations,
};
pub use describe_table::{IcebergDescribeTableInput, IcebergDescribeTableTool};
pub use list_tables::{IcebergListTablesInput, IcebergListTablesTool};
pub use namespace::{
    IcebergCreateNamespaceInput, IcebergCreateNamespaceTool, IcebergDropNamespaceInput,
    IcebergDropNamespaceTool, IcebergListNamespacesInput, IcebergListNamespacesTool,
    IcebergNamespaceExistsInput, IcebergNamespaceExistsTool,
};
pub use query::{IcebergPreviewTableInput, IcebergPreviewTableTool};
pub use table::{
    IcebergCreateTableInput, IcebergCreateTableTool, IcebergDropTableInput,
    IcebergDropTableTool, IcebergListTablesInNamespaceInput, IcebergListTablesInNamespaceTool,
    IcebergLoadTableInput, IcebergLoadTableTool, IcebergRenameTableInput,
    IcebergRenameTableTool, IcebergTableExistsInput, IcebergTableExistsTool,
};

/// Serialize an Iceberg [`Namespace`] into a JSON object.
fn ns_to_json(ns: &Namespace, already_exists: bool) -> Value {
    json!({
        "namespace": ns.name().as_ref().join("."),
        "properties": ns.properties(),
        "already_exists": already_exists,
    })
}

/// All aether tool registrations, ready to register into a toolset.
///
/// Returns 13 tools:
/// - **DataFusion** (2): `iceberg_list_tables`, `iceberg_describe_table`
/// - **Namespace CRUD** (4): `iceberg_list_namespaces`, `iceberg_create_namespace`,
///   `iceberg_namespace_exists`, `iceberg_drop_namespace`
/// - **Table CRUD** (6): `iceberg_list_tables_in_namespace`, `iceberg_table_exists`,
///   `iceberg_load_table`, `iceberg_create_table`, `iceberg_drop_table`,
///   `iceberg_rename_table`
/// - **SQL preview** (1): `iceberg_preview_table`
pub fn iceberg_registrations(workspace: Arc<DataSession>) -> Vec<ToolRegistration> {
    vec![
        // DataFusion-level tools
        ToolRegistration::from(IcebergListTablesTool {
            workspace: workspace.clone(),
        }),
        ToolRegistration::from(IcebergDescribeTableTool {
            workspace: workspace.clone(),
        }),
        // namespace tools
        ToolRegistration::from(IcebergListNamespacesTool {
            workspace: workspace.clone(),
        }),
        ToolRegistration::from(IcebergCreateNamespaceTool {
            workspace: workspace.clone(),
        }),
        ToolRegistration::from(IcebergNamespaceExistsTool {
            workspace: workspace.clone(),
        }),
        ToolRegistration::from(IcebergDropNamespaceTool {
            workspace: workspace.clone(),
        }),
        // table tools
        ToolRegistration::from(IcebergListTablesInNamespaceTool {
            workspace: workspace.clone(),
        }),
        ToolRegistration::from(IcebergTableExistsTool {
            workspace: workspace.clone(),
        }),
        ToolRegistration::from(IcebergLoadTableTool {
            workspace: workspace.clone(),
        }),
        ToolRegistration::from(IcebergCreateTableTool {
            workspace: workspace.clone(),
        }),
        ToolRegistration::from(IcebergDropTableTool {
            workspace: workspace.clone(),
        }),
        ToolRegistration::from(IcebergRenameTableTool {
            workspace: workspace.clone(),
        }),
        // read tool
        ToolRegistration::from(IcebergPreviewTableTool { workspace }),
    ]
}
