use std::sync::Arc;

use anyhow::Result;
use serde_json::{Value, json};

use agentik_sdk::types::{ToolResult, ToolResultContent};
use eutils_rs::EutilsClient;

mod common;

/// Extract content of a `ToolResult` as a string.
fn content_str(result: &ToolResult) -> String {
    match &result.content {
        ToolResultContent::Text(s) => s.clone(),
        ToolResultContent::Json(v) => serde_json::to_string(v).unwrap_or_default(),
        ToolResultContent::Blocks(blocks) => blocks
            .iter()
            .filter_map(|b| match b {
                agentik_sdk::types::ToolResultBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

async fn run_tool(
    tool: &dyn agentik_core::tools::DynToolFunction,
    input: Value,
) -> Result<ToolResult> {
    tool.execute(input)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))
}

#[tokio::test]
#[ignore]
#[common::serial]
async fn tool_pubmed_search() -> Result<()> {
    let client = Arc::new(common::test_client());
    let registrations = eutils_rs::eutils_registrations(client);

    let tool = registrations
        .iter()
        .find(|t| t.definition.name == "pubmed_search")
        .expect("pubmed_search not registered")
        .implementation
        .clone();

    let result = run_tool(
        &*tool,
        json!({ "term": "CRISPR[Title/Abstract]", "retmax": 3 }),
    )
    .await?;
    let content = content_str(&result);
    assert!(content.contains("\"count\""));
    assert!(content.contains("\"id_list\""));

    Ok(())
}

#[tokio::test]
#[ignore]
#[common::serial]
async fn tool_pubmed_fetch() -> Result<()> {
    let client = Arc::new(common::test_client());
    let registrations = eutils_rs::eutils_registrations(client);

    let tool = registrations
        .iter()
        .find(|t| t.definition.name == "pubmed_fetch")
        .expect("pubmed_fetch not registered")
        .implementation
        .clone();

    common::rate_limit();
    let result = run_tool(
        &*tool,
        json!({
            "pmid": common::PMID_CRISPR,
            "rettype": "abstract",
            "retmode": "text"
        }),
    )
    .await?;
    let content = content_str(&result);
    assert!(content.contains("PMID"));

    Ok(())
}

#[tokio::test]
#[ignore]
#[common::serial]
async fn tool_pubmed_summary() -> Result<()> {
    let client = Arc::new(common::test_client());
    let registrations = eutils_rs::eutils_registrations(client);

    let tool = registrations
        .iter()
        .find(|t| t.definition.name == "pubmed_summary")
        .expect("pubmed_summary not registered")
        .implementation
        .clone();

    common::rate_limit();
    let result = run_tool(&*tool, json!({ "pmid": common::PMID_CRISPR })).await?;
    let content = content_str(&result);
    assert!(content.contains("title"));

    Ok(())
}

#[tokio::test]
#[ignore]
#[common::serial]
async fn tool_pubmed_related() -> Result<()> {
    let client = Arc::new(common::test_client());
    let registrations = eutils_rs::eutils_registrations(client);

    let tool = registrations
        .iter()
        .find(|t| t.definition.name == "pubmed_related")
        .expect("pubmed_related not registered")
        .implementation
        .clone();

    common::rate_limit();
    let result = run_tool(&*tool, json!({ "pmid": common::PMID_CRISPR })).await?;
    let content = content_str(&result);
    assert!(content.contains("linksets"));

    Ok(())
}

#[tokio::test]
#[ignore]
#[common::serial]
async fn tool_pubmed_spell() -> Result<()> {
    let client = Arc::new(common::test_client());
    let registrations = eutils_rs::eutils_registrations(client);

    let tool = registrations
        .iter()
        .find(|t| t.definition.name == "pubmed_spell")
        .expect("pubmed_spell not registered")
        .implementation
        .clone();

    if !common::espell_available().await {
        return Ok(());
    }
    common::rate_limit();
    let result = run_tool(&*tool, json!({ "term": "onkogene" })).await?;
    let content = content_str(&result);
    assert!(content.contains("OriginalQuery"));

    Ok(())
}

#[tokio::test]
#[ignore]
#[common::serial]
async fn tool_einfo() -> Result<()> {
    let client = Arc::new(common::test_client());
    let registrations = eutils_rs::eutils_registrations(client);

    let tool = registrations
        .iter()
        .find(|t| t.definition.name == "einfo")
        .expect("einfo not registered")
        .implementation
        .clone();

    common::rate_limit();
    let result = run_tool(&*tool, json!({ "db": "pubmed" })).await?;
    let content = content_str(&result);
    assert!(content.contains("\"count\""));

    Ok(())
}

#[tokio::test]
#[ignore]
#[common::serial]
async fn tool_egquery() -> Result<()> {
    let client = Arc::new(common::test_client());
    let registrations = eutils_rs::eutils_registrations(client);

    let tool = registrations
        .iter()
        .find(|t| t.definition.name == "egquery")
        .expect("egquery not registered")
        .implementation
        .clone();

    if !common::egquery_available().await {
        return Ok(());
    }
    common::rate_limit();
    let result = run_tool(&*tool, json!({ "term": "CRISPR" })).await?;
    let content = content_str(&result);
    assert!(content.contains("\"dbname\""));

    Ok(())
}

#[test]
#[ignore]
fn tool_registrations_count() {
    let client = Arc::new(common::test_client());
    let registrations = eutils_rs::eutils_registrations(client);
    assert_eq!(registrations.len(), 7);

    let names: Vec<&str> = registrations
        .iter()
        .map(|t| t.definition.name.as_str())
        .collect();
    assert!(names.contains(&"pubmed_search"));
    assert!(names.contains(&"pubmed_fetch"));
    assert!(names.contains(&"pubmed_summary"));
    assert!(names.contains(&"pubmed_related"));
    assert!(names.contains(&"pubmed_spell"));
    assert!(names.contains(&"einfo"));
    assert!(names.contains(&"egquery"));
}

#[test]
#[ignore]
fn tool_definitions_have_schemas() {
    let client = Arc::new(common::test_client());
    let registrations = eutils_rs::eutils_registrations(client);

    for reg in &registrations {
        assert!(!reg.definition.name.is_empty());
        assert!(!reg.definition.description.is_empty());
        assert!(!reg.definition.input_schema.schema_type.is_empty());
        assert!(!reg.definition.input_schema.properties.is_empty());
    }
}
