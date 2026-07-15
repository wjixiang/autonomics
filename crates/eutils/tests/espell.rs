use anyhow::Result;
use eutils::EutilsClient;

mod common;

#[tokio::test]
#[ignore]
#[common::serial]
async fn espell_correct_spelling() -> Result<()> {
    if !common::espell_available().await {
        return Ok(());
    }
    let client = common::test_client();
    common::rate_limit();
    let resp = client.espell("pubmed", "CRISPR").await?;

    assert!(resp.get("OriginalQuery").is_some());
    assert_eq!(
        resp["OriginalQuery"]["term"]
            .as_str()
            .unwrap_or("")
            .to_lowercase(),
        "crispr"
    );
    assert!(resp.get("CorrectedQuery").is_none());

    Ok(())
}

#[tokio::test]
#[ignore]
#[common::serial]
async fn espell_misspelled_term() -> Result<()> {
    if !common::espell_available().await {
        return Ok(());
    }
    let client = common::test_client();
    common::rate_limit();
    let resp = client.espell("pubmed", "onkogene").await?;

    assert!(resp.get("OriginalQuery").is_some());
    assert!(resp.get("CorrectedQuery").is_some());
    assert!(
        !resp["CorrectedQuery"]["term"]
            .as_str()
            .unwrap_or("")
            .is_empty()
    );

    Ok(())
}

#[tokio::test]
#[ignore]
#[common::serial]
async fn espell_response_structure() -> Result<()> {
    if !common::espell_available().await {
        return Ok(());
    }
    let client = common::test_client();
    common::rate_limit();
    let resp = client.espell("pubmed", "CRISPR").await?;

    assert!(resp.get("db").is_some());
    assert_eq!(resp["db"].as_str(), Some("pubmed"));

    Ok(())
}
