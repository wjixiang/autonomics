use anyhow::Result;
use eutils_rs::types::ESummaryRequest;
use eutils_rs::EutilsClient;

mod common;

#[tokio::test]
#[common::serial]
async fn esummary_single_pmid_v2() -> Result<()> {
    let client = common::test_client();
    common::rate_limit();
    let req = ESummaryRequest {
        db: "pubmed".into(),
        id: common::PMID_CRISPR.into(),
        version: Some("2.0".into()),
        ..Default::default()
    };
    let resp = client.esummary(&req).await?;

    let result = resp.get("result").expect("missing 'result' key");
    let uids = result.get("uids").and_then(|v| v.as_array()).expect("missing 'uids'");
    assert!(uids.len() >= 1, "expected at least 1 uid");

    let article = result.get(common::PMID_CRISPR).expect("missing article by PMID");
    assert!(article.is_object());
    assert!(article.get("title").is_some());
    assert!(article.get("authors").is_some());
    assert!(article.get("source").is_some());
    assert!(article.get("pubdate").is_some());

    let title = article["title"].as_str().unwrap_or("");
    assert!(!title.is_empty());

    Ok(())
}

#[tokio::test]
#[common::serial]
async fn esummary_multiple_pmids() -> Result<()> {
    let client = common::test_client();
    common::rate_limit();
    let ids = format!("{},{}", common::PMID_CRISPR, common::PMID_BRCA1);
    let req = ESummaryRequest {
        db: "pubmed".into(),
        id: ids,
        version: Some("2.0".into()),
        retmax: Some(10),
        ..Default::default()
    };
    let resp = client.esummary(&req).await?;

    let uids = resp["result"]["uids"].as_array().expect("missing 'uids'");
    assert_eq!(uids.len(), 2);

    Ok(())
}

#[tokio::test]
#[common::serial]
async fn esummary_invalid_pmid_returns_empty() -> Result<()> {
    let client = common::test_client();
    common::rate_limit();
    let req = ESummaryRequest {
        db: "pubmed".into(),
        id: "00000000".into(),
        version: Some("2.0".into()),
        ..Default::default()
    };
    let resp = client.esummary(&req).await?;

    let has_ids = resp["result"]["uids"].as_array().map_or(false, |arr| !arr.is_empty());
    assert!(!has_ids, "invalid PMID should not return real records");

    Ok(())
}

#[tokio::test]
#[common::serial]
async fn esummary_article_has_expected_fields() -> Result<()> {
    let client = common::test_client();
    common::rate_limit();
    let req = ESummaryRequest {
        db: "pubmed".into(),
        id: common::PMID_RNA_SEQ.into(),
        version: Some("2.0".into()),
        ..Default::default()
    };
    let resp = client.esummary(&req).await?;
    let article = &resp["result"][common::PMID_RNA_SEQ];

    for field in &["uid", "title", "pubdate", "epubdate", "source", "authors", "fulljournalname"] {
        assert!(article.get(*field).is_some(), "missing field: {}", field);
    }

    Ok(())
}
