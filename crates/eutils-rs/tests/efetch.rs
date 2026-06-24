use anyhow::Result;
use eutils_rs::types::EFetchRequest;
use eutils_rs::EutilsClient;

mod common;

#[tokio::test]
#[common::serial]
async fn efetch_abstract_single_pmid() -> Result<()> {
    let client = common::test_client();
    common::rate_limit();
    let req = EFetchRequest {
        db: "pubmed".into(),
        id: common::PMID_CRISPR.into(),
        rettype: Some("abstract".into()),
        retmode: Some("text".into()),
        ..Default::default()
    };
    let text = client.efetch(&req).await?;

    assert!(text.len() > 50, "abstract too short: {} bytes", text.len());
    assert!(text.contains("PMID"));
    assert!(!text.contains("ID list is empty"));

    Ok(())
}

#[tokio::test]
#[common::serial]
async fn efetch_medline_format() -> Result<()> {
    let client = common::test_client();
    common::rate_limit();
    let req = EFetchRequest {
        db: "pubmed".into(),
        id: common::PMID_CRISPR.into(),
        rettype: Some("medline".into()),
        retmode: Some("text".into()),
        ..Default::default()
    };
    let text = client.efetch(&req).await?;

    assert!(text.contains("PMID- "));
    assert!(text.contains("TI  -") || text.contains("DP  -"));

    Ok(())
}

#[tokio::test]
#[common::serial]
async fn efetch_multiple_pmids() -> Result<()> {
    let client = common::test_client();
    common::rate_limit();
    let ids = format!("{},{}", common::PMID_CRISPR, common::PMID_BRCA1);
    let req = EFetchRequest {
        db: "pubmed".into(),
        id: ids,
        rettype: Some("abstract".into()),
        retmode: Some("text".into()),
        retmax: Some(10),
        ..Default::default()
    };
    let text = client.efetch(&req).await?;

    assert!(text.contains(common::PMID_CRISPR));
    assert!(text.contains(common::PMID_BRCA1));

    Ok(())
}

#[tokio::test]
#[common::serial]
async fn efetch_invalid_pmid() {
    let client = common::test_client();
    common::rate_limit();
    let req = EFetchRequest {
        db: "pubmed".into(),
        id: "00000000".into(),
        rettype: Some("abstract".into()),
        retmode: Some("text".into()),
        ..Default::default()
    };
    // Invalid PMIDs should return an error (HTTP 400 or similar).
    let result = client.efetch(&req).await;
    assert!(result.is_err(), "expected error for invalid PMID");
}

#[tokio::test]
#[common::serial]
async fn efetch_default_text_mode() -> Result<()> {
    let client = common::test_client();
    common::rate_limit();
    let req = EFetchRequest {
        db: "pubmed".into(),
        id: common::PMID_CRISPR.into(),
        ..Default::default()
    };
    let text = client.efetch(&req).await?;

    assert!(!text.is_empty());
    assert!(text.contains("PMID"));

    Ok(())
}
