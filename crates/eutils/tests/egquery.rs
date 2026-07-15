use anyhow::Result;
use eutils::EutilsClient;

mod common;

#[tokio::test]
#[ignore]
#[common::serial]
async fn egquery_cross_database_search() -> Result<()> {
    if !common::egquery_available().await {
        return Ok(());
    }
    let client = common::test_client();
    common::rate_limit();

    let resp = client.egquery("CRISPR").await?;

    assert!(!resp.result.is_empty());

    let pubmed = resp
        .result
        .iter()
        .find(|db| db["dbname"].as_str() == Some("pubmed"))
        .expect("PubMed not in EGQuery results");

    let count: u64 = pubmed["count"].as_str().unwrap_or("0").parse().unwrap_or(0);
    assert!(count > 10_000, "PubMed CRISPR count > 10k, got {}", count);

    for entry in &resp.result {
        assert!(entry.get("dbname").is_some());
        assert!(entry.get("count").is_some());
    }

    Ok(())
}

#[tokio::test]
#[ignore]
#[common::serial]
async fn egquery_niche_term() -> Result<()> {
    if !common::egquery_available().await {
        return Ok(());
    }
    let client = common::test_client();
    common::rate_limit();

    let resp = client.egquery("MendelianRandomization").await?;

    assert!(!resp.result.is_empty());

    let pubmed = resp
        .result
        .iter()
        .find(|db| db["dbname"].as_str() == Some("pubmed"));
    assert!(pubmed.is_some());

    let count: u64 = pubmed.unwrap()["count"]
        .as_str()
        .unwrap_or("0")
        .parse()
        .unwrap_or(0);
    assert!(count > 1_000, "expected > 1k MR results, got {}", count);

    Ok(())
}

#[tokio::test]
#[ignore]
#[common::serial]
async fn egquery_no_matches() -> Result<()> {
    if !common::egquery_available().await {
        return Ok(());
    }
    let client = common::test_client();
    common::rate_limit();

    let resp = client.egquery("zzzznonexistenttermxyz123456").await?;

    assert!(!resp.result.is_empty());

    Ok(())
}
