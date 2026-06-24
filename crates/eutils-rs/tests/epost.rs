use anyhow::Result;
use eutils_rs::EutilsClient;

mod common;

#[tokio::test]
#[common::serial]
async fn epost_and_chain_to_efetch() -> Result<()> {
    let client = common::test_client();
    common::rate_limit();

    let ids = vec![
        common::PMID_CRISPR.to_string(),
        common::PMID_BRCA1.to_string(),
    ];
    let (query_key, web_env) = client.epost("pubmed", &ids).await?;

    assert!(!query_key.is_empty());
    assert!(web_env.len() > 10);

    common::rate_limit();

    use eutils_rs::types::EFetchRequest;
    let req = EFetchRequest {
        db: "pubmed".into(),
        id: String::new(),
        rettype: Some("abstract".into()),
        retmode: Some("text".into()),
        web_env: Some(web_env),
        query_key: Some(query_key),
        ..Default::default()
    };
    let text = client.efetch(&req).await?;

    assert!(text.contains(common::PMID_CRISPR), "should contain first PMID");
    assert!(text.contains(common::PMID_BRCA1), "should contain second PMID");

    Ok(())
}

#[tokio::test]
#[common::serial]
async fn epost_returns_valid_key_and_webenv() -> Result<()> {
    let client = common::test_client();
    common::rate_limit();
    let ids = vec![common::PMID_CRISPR.to_string()];
    let (query_key, web_env) = client.epost("pubmed", &ids).await?;

    let key_num: u32 = query_key.parse().expect("query_key should be integer");
    assert_eq!(key_num, 1);
    assert!(web_env.len() > 20, "web_env too short: len={}", web_env.len());

    Ok(())
}
