use anyhow::Result;
use eutils::EutilsClient;

mod common;

#[tokio::test]
#[ignore]
#[common::serial]
async fn einfo_lists_all_databases() -> Result<()> {
    let client = common::test_client();
    let resp = client.einfo(None).await?;

    let dblist = resp.result.dblist.unwrap_or_default();
    assert!(
        dblist.len() > 20,
        "expected 20+ databases, got {}",
        dblist.len()
    );
    assert!(
        dblist.iter().any(|d| d == "pubmed"),
        "pubmed not in database list"
    );
    assert!(
        dblist.iter().any(|d| d == "gene"),
        "gene not in database list"
    );
    assert!(
        dblist.iter().any(|d| d == "nucleotide"),
        "nucleotide not in database list"
    );

    Ok(())
}

#[tokio::test]
#[ignore]
#[common::serial]
async fn einfo_single_database() -> Result<()> {
    let client = common::test_client();
    common::rate_limit();
    let resp = client.einfo(Some("pubmed")).await?;

    let dbinfo = resp.result.dbinfo.expect("expected dbinfo array");
    assert_eq!(dbinfo.len(), 1, "expected exactly one dbinfo entry");

    let info = &dbinfo[0];
    assert_eq!(info.dbname, "pubmed");

    let n: u64 = info.count.as_deref().unwrap_or("0").parse().unwrap_or(0);
    assert!(
        n > 35_000_000,
        "PubMed should have > 35M records, got {}",
        n
    );

    Ok(())
}

#[tokio::test]
#[ignore]
#[common::serial]
async fn einfo_gene_database() -> Result<()> {
    let client = common::test_client();
    common::rate_limit();
    let resp = client.einfo(Some("gene")).await?;

    let dbinfo = resp.result.dbinfo.expect("expected dbinfo array");
    let info = &dbinfo[0];
    assert_eq!(info.dbname, "gene");

    let n: u64 = info.count.as_deref().unwrap_or("0").parse().unwrap_or(0);
    assert!(n > 50_000, "Gene should have > 50k records, got {}", n);

    Ok(())
}

#[tokio::test]
#[ignore]
#[common::serial]
async fn einfo_invalid_database_returns_error() {
    let client = common::test_client();
    common::rate_limit();
    let result = client.einfo(Some("nonexistent_db_xyz")).await;
    assert!(result.is_err(), "expected error for invalid database");
}
