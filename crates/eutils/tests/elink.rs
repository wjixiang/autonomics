use eutils::EutilsClient;
use eutils::types::ELinkRequest;

type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
mod common;

#[tokio::test]
#[ignore]
#[common::serial]
async fn elink_related_articles() -> TestResult<()> {
    let client = common::test_client();
    common::rate_limit();
    let req = ELinkRequest {
        dbfrom: "pubmed".into(),
        id: common::PMID_CRISPR.into(),
        db: Some("pubmed".into()),
        cmd: Some("neighbor".into()),
        ..Default::default()
    };
    let resp = client.elink(&req).await?;

    let linksets = resp
        .get("linksets")
        .and_then(|v| v.as_array())
        .expect("missing linksets");
    assert!(!linksets.is_empty());

    let linkdbs = linksets[0]
        .get("linksetdbs")
        .and_then(|v| v.as_array())
        .expect("missing linksetdbs");
    assert!(!linkdbs.is_empty());
    assert!(linkdbs[0].get("links").is_some());

    Ok(())
}

#[tokio::test]
#[ignore]
#[common::serial]
async fn elink_gene_to_pubmed() -> TestResult<()> {
    let client = common::test_client();
    common::rate_limit();
    let req = ELinkRequest {
        dbfrom: "gene".into(),
        id: "672".into(),
        db: Some("pubmed".into()),
        cmd: Some("neighbor".into()),
        ..Default::default()
    };
    let resp = client.elink(&req).await?;

    let linksets = resp["linksets"].as_array().expect("missing linksets");
    assert!(!linksets.is_empty());

    Ok(())
}

#[tokio::test]
#[ignore]
#[common::serial]
async fn elink_neighbor_history() -> TestResult<()> {
    let client = common::test_client();
    common::rate_limit();
    let req = ELinkRequest {
        dbfrom: "pubmed".into(),
        id: common::PMID_CRISPR.into(),
        db: Some("pubmed".into()),
        cmd: Some("neighbor_history".into()),
        ..Default::default()
    };
    let resp = client.elink(&req).await?;

    let linksets = resp["linksets"].as_array().expect("missing linksets");
    assert!(!linksets.is_empty());

    Ok(())
}

#[tokio::test]
#[ignore]
#[common::serial]
async fn elink_multiple_pmids() -> TestResult<()> {
    let client = common::test_client();
    common::rate_limit();
    let ids = format!("{},{}", common::PMID_CRISPR, common::PMID_BRCA1);
    let req = ELinkRequest {
        dbfrom: "pubmed".into(),
        id: ids,
        db: Some("pubmed".into()),
        cmd: Some("neighbor".into()),
        ..Default::default()
    };
    let resp = client.elink(&req).await?;

    let linksets = resp["linksets"].as_array().expect("missing linksets");
    assert_eq!(linksets.len(), 2, "expected 2 link sets");

    Ok(())
}
