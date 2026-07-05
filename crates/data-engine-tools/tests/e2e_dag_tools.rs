use std::sync::Arc;

use agentik_core::tools::Toolset;
use agentik_sdk::types::tools::{ToolUse, ToolResult};
use data_engine::data_engine::DataEngine;
use data_engine::runtime::spawn_with_engine;
use fs::OpendalFileStorage;
use serde_json::json;

fn build_tooluse(id: &str, name: &str, input: serde_json::Value) -> ToolUse {
    ToolUse {
        id: id.to_string(),
        name: name.to_string(),
        input,
    }
}

fn check_ok(result: &ToolResult, label: &str) {
    assert!(!result.is_error.unwrap_or(false), "{label} failed: {:?}", result.content);
}

#[tokio::test]
async fn test_add_source_sql_run_dag() {
    // 1. Set up file storage and write test data
    let file_storage = Arc::new(OpendalFileStorage::new_in_fs());
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let csv_path = std::path::Path::new(&manifest_dir)
        .join("../data-engine/test_datasets/insurance.csv");
    let csv_data = std::fs::read(csv_path).unwrap();
    file_storage
        .op
        .write("/insurance.csv", csv_data)
        .await
        .unwrap();

    // 2. Build DataEngine and spawn server
    let engine = DataEngine::builder()
        .register_opendal_fs(file_storage)
        .unwrap()
        .build();
    let (client, _handle) = spawn_with_engine(engine);

    // 3. Register tools
    let tools = data_engine_tools::registrations(Arc::new(client.clone()));
    let mut toolset = Toolset::new(None);
    toolset.register_all(tools).unwrap();

    // 4. Add source node
    let results = toolset
        .execute(
            &[build_tooluse("tc1", "add_source_node", json!({"id": "src", "path": "/insurance.csv"}))],
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    check_ok(&results[0], "add_source_node");

    // 5. Add SQL node
    let results = toolset
        .execute(
            &[build_tooluse("tc2", "add_sql_node", json!({
                "id": "sql",
                "query": "SELECT age, charges FROM src WHERE age > 30 LIMIT 5"
            }))],
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    check_ok(&results[0], "add_sql_node");

    // 6. Add edge (src -> sql) via tool
    let results = toolset
        .execute(
            &[build_tooluse("tc3", "add_edge", json!({"from": "src", "to": "sql"}))],
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    check_ok(&results[0], "add_edge");

    // 7. Add sink node
    let results = toolset
        .execute(
            &[build_tooluse("tc4", "add_sink_node", json!({
                "id": "sink",
                "path": "/output.csv",
                "format": "csv"
            }))],
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    check_ok(&results[0], "add_sink_node");

    // 8. Edge (sql -> sink) via tool
    let results = toolset
        .execute(
            &[build_tooluse("tc5", "add_edge", json!({"from": "sql", "to": "sink"}))],
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    check_ok(&results[0], "add_edge sql->sink");
}
