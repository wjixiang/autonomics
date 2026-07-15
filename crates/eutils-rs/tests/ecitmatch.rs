use anyhow::Result;
use eutils_rs::EutilsClient;
use eutils_rs::types::ECitMatchRequest;

mod common;

#[tokio::test]
#[ignore]
#[common::serial]
async fn ecitmatch_known_article() -> Result<()> {
    let client = common::test_client();
    common::rate_limit();

    let citation = "Nature communications|9|2069|2018|Ma H|Correction of a pathogenic gene mutation in human embryos".to_string();
    let req = ECitMatchRequest {
        bdata: vec![citation],
    };
    let resp = client.ecitmatch(&req).await?;

    assert!(resp.contains("PMID"), "response should contain PMID");

    Ok(())
}

#[tokio::test]
#[ignore]
#[common::serial]
async fn ecitmatch_batch_citations() -> Result<()> {
    let client = common::test_client();
    common::rate_limit();

    let citations = vec![
        "Science|361|361|2018||".to_string(),
        "Nature|556|43|2018||".to_string(),
    ];
    let req = ECitMatchRequest { bdata: citations };
    let resp = client.ecitmatch(&req).await?;

    assert!(
        resp.contains("<?xml") || resp.contains("PMID") || resp.is_empty(),
        "should return XML or be empty"
    );

    Ok(())
}

#[tokio::test]
#[ignore]
#[common::serial]
async fn ecitmatch_empty_batch() -> Result<()> {
    let client = common::test_client();
    common::rate_limit();
    let req = ECitMatchRequest { bdata: vec![] };
    let resp = client.ecitmatch(&req).await?;

    // Empty batch should not crash; response may be empty, XML, or a JSON error.
    // Just verify we got a response without error.
    let _ = &resp;

    Ok(())
}
