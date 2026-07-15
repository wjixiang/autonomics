use eutils::EutilsClient;
use eutils::types::ESearchRequest;

type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
mod common;

#[tokio::test]
#[ignore]
#[common::serial]
async fn esearch_basic_pubmed_search() -> TestResult<()> {
    let client = common::test_client();
    let resp = client
        .esearch(&ESearchRequest::new("pubmed", "CRISPR"))
        .await?;

    let n: u64 = resp.result.count.parse().unwrap_or(0);
    assert!(n > 10_000, "expected > 10k CRISPR results, got {}", n);
    assert!(
        !resp.result.id_list.is_empty(),
        "id_list should not be empty"
    );
    assert!(resp.result.id_list.len() <= 20);

    Ok(())
}

#[tokio::test]
#[ignore]
#[common::serial]
async fn esearch_with_retmax_pagination() -> TestResult<()> {
    let client = common::test_client();
    common::rate_limit();
    let req = ESearchRequest {
        db: "pubmed".into(),
        term: "cancer".into(),
        retmax: Some(5),
        retstart: Some(0),
        ..Default::default()
    };
    let resp = client.esearch(&req).await?;
    assert_eq!(resp.result.id_list.len(), 5);
    assert_eq!(&resp.result.retstart, "0");

    Ok(())
}

#[tokio::test]
#[ignore]
#[common::serial]
async fn esearch_retstart_offset() -> TestResult<()> {
    let client = common::test_client();
    common::rate_limit();
    let req = ESearchRequest {
        db: "pubmed".into(),
        term: "CRISPR".into(),
        retmax: Some(5),
        retstart: Some(5),
        ..Default::default()
    };
    let resp = client.esearch(&req).await?;
    assert_eq!(resp.result.id_list.len(), 5);
    assert_eq!(&resp.result.retstart, "5");

    Ok(())
}

#[tokio::test]
#[ignore]
#[common::serial]
async fn esearch_with_usehistory() -> TestResult<()> {
    let client = common::test_client();
    common::rate_limit();
    let req = ESearchRequest {
        db: "pubmed".into(),
        term: "CRISPR".into(),
        retmax: Some(5),
        usehistory: Some(true),
        ..Default::default()
    };
    let resp = client.esearch(&req).await?;

    assert!(resp.result.query_key.is_some(), "expected query_key");
    assert!(resp.result.web_env.is_some(), "expected web_env");
    let wk = resp.result.web_env.unwrap();
    assert!(wk.len() > 10, "web_env too short: len={}", wk.len());

    Ok(())
}

#[tokio::test]
#[ignore]
#[common::serial]
async fn esearch_date_filter() -> TestResult<()> {
    let client = common::test_client();
    common::rate_limit();
    let req = ESearchRequest {
        db: "pubmed".into(),
        term: "cancer".into(),
        retmax: Some(5),
        datetype: Some("pdat".into()),
        mindate: Some("2024/01/01".into()),
        maxdate: Some("2024/12/31".into()),
        ..Default::default()
    };
    let resp = client.esearch(&req).await?;

    let n: u64 = resp.result.count.parse().unwrap_or(0);
    assert!(n > 0, "expected results for 2024 cancer search");

    Ok(())
}

#[tokio::test]
#[ignore]
#[common::serial]
async fn esearch_reldate_filter() -> TestResult<()> {
    let client = common::test_client();
    common::rate_limit();
    let req = ESearchRequest {
        db: "pubmed".into(),
        term: "COVID-19".into(),
        retmax: Some(5),
        datetype: Some("pdat".into()),
        reldate: Some(30),
        ..Default::default()
    };
    let resp = client.esearch(&req).await?;

    let n: u64 = resp.result.count.parse().unwrap_or(0);
    assert!(n > 0, "expected recent COVID-19 results");

    Ok(())
}

#[tokio::test]
#[ignore]
#[common::serial]
async fn esearch_sort_pub_date() -> TestResult<()> {
    let client = common::test_client();
    common::rate_limit();
    let req = ESearchRequest {
        db: "pubmed".into(),
        term: "cancer".into(),
        retmax: Some(5),
        sort: Some("pub_date".into()),
        ..Default::default()
    };
    let resp = client.esearch(&req).await?;
    assert_eq!(resp.result.id_list.len(), 5);

    Ok(())
}

#[tokio::test]
#[ignore]
#[common::serial]
async fn esearch_empty_result() -> TestResult<()> {
    let client = common::test_client();
    common::rate_limit();
    let resp = client
        .esearch(&ESearchRequest::new(
            "pubmed",
            "zzzznonexistenttermxyz123456[Title]",
        ))
        .await?;

    let n: u64 = resp.result.count.parse().unwrap_or(0);
    assert_eq!(n, 0);
    assert!(resp.result.id_list.is_empty());

    Ok(())
}

#[tokio::test]
#[ignore]
#[common::serial]
async fn esearch_specific_pmid_query() -> TestResult<()> {
    let client = common::test_client();
    common::rate_limit();
    let resp = client
        .esearch(&ESearchRequest::new("pubmed", common::PMID_CRISPR))
        .await?;

    assert_eq!(&resp.result.count, "1");
    assert_eq!(
        resp.result.id_list.first().map(|s| s.as_str()),
        Some(common::PMID_CRISPR)
    );

    Ok(())
}

#[tokio::test]
#[ignore]
#[common::serial]
async fn esearch_two_pages_dont_overlap() -> TestResult<()> {
    let client = common::test_client();
    common::rate_limit();

    let req1 = ESearchRequest {
        db: "pubmed".into(),
        term: "cancer immunotherapy[Title/Abstract]".into(),
        retmax: Some(5),
        retstart: Some(0),
        ..Default::default()
    };
    let resp1 = client.esearch(&req1).await?;

    common::rate_limit();
    let req2 = ESearchRequest {
        db: "pubmed".into(),
        term: "cancer immunotherapy[Title/Abstract]".into(),
        retmax: Some(5),
        retstart: Some(5),
        ..Default::default()
    };
    let resp2 = client.esearch(&req2).await?;

    let page1: std::collections::HashSet<&str> =
        resp1.result.id_list.iter().map(|s| s.as_str()).collect();
    let page2: std::collections::HashSet<&str> =
        resp2.result.id_list.iter().map(|s| s.as_str()).collect();
    assert!(
        page1.intersection(&page2).count() == 0,
        "pages should not overlap"
    );

    Ok(())
}
