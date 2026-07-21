use opengwas::*;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build an authenticated client. Skips the test if no token is available.
fn require_client() -> Option<OpengwasClient> {
    if std::env::var("OPENGWAS_TOKEN").is_err() {
        eprintln!("OPENGWAS_TOKEN not set — skipping authenticated test");
        return None;
    }
    Some(OpengwasClient::new(None).expect("opengwas client"))
}

// ---------------------------------------------------------------------------
// Public endpoints (no auth required)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_status() {
    let client = OpengwasClient::new_no_auth().expect("opengwas client");
    let resp = client.status().await.unwrap();
    // The API returns a JSON object with service statuses.
    assert!(resp.is_object());
}

#[tokio::test]
async fn test_batches() {
    let client = OpengwasClient::new_no_auth().expect("opengwas client");
    let resp = client.batches().await.unwrap();
    assert!(resp.is_array() || resp.is_object());
}

// ---------------------------------------------------------------------------
// User (auth required)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_user() {
    let Some(client) = require_client() else {
        return;
    };
    let resp = client.user().await.unwrap();
    assert!(resp.is_object());
}

// ---------------------------------------------------------------------------
// GwasInfo — SQLite caching
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_gwasinfo_all_caches() {
    let Some(client) = require_client() else {
        return;
    };

    // First call — fetches from remote.
    let all = client.gwasinfo_all().await.unwrap();
    assert!(
        !all.is_empty(),
        "gwasinfo_all should return at least one dataset"
    );
    println!("Total datasets cached: {}", all.len());

    // Verify the first entry has an id.
    let first = &all[0];
    assert!(first.id.is_some(), "dataset id should be present");
    println!("First dataset: id={:?}, trait={:?}", first.id, first.trait_);
}

#[tokio::test]
async fn test_gwasinfo_count() {
    let Some(client) = require_client() else {
        return;
    };

    let count = client.gwasinfo_count().await.unwrap();
    assert!(count > 0, "should have at least one dataset");
    println!("gwasinfo_count: {count}");
}

#[tokio::test]
async fn test_gwasinfo_count_matches_all() {
    let Some(client) = require_client() else {
        return;
    };

    // Both should agree since they read from the same cache.
    let all = client.gwasinfo_all().await.unwrap();
    let count = client.gwasinfo_count().await.unwrap();
    assert_eq!(all.len() as i64, count);
}

#[tokio::test]
async fn test_gwasinfo_by_id() {
    let Some(client) = require_client() else {
        return;
    };

    // Grab a known ID from the full list.
    let all = client.gwasinfo_all().await.unwrap();
    let known_id = all[0].id.clone().unwrap();

    let result = client
        .gwasinfo(&GwasInfoRequest {
            id: vec![known_id.clone()],
        })
        .await
        .unwrap();
    assert_eq!(result.len(), 1, "should find exactly one match");
    assert_eq!(result[0].id.as_deref(), Some(known_id.as_str()));
}

#[tokio::test]
async fn test_gwasinfo_multiple_ids() {
    let Some(client) = require_client() else {
        return;
    };

    let all = client.gwasinfo_all().await.unwrap();
    if all.len() < 3 {
        println!("Not enough datasets to test multi-ID query, skipping");
        return;
    }
    let ids: Vec<String> = all.iter().take(3).filter_map(|g| g.id.clone()).collect();

    let result = client
        .gwasinfo(&GwasInfoRequest { id: ids.clone() })
        .await
        .unwrap();
    assert_eq!(result.len(), ids.len());
    for gwas in &result {
        assert!(ids.contains(&gwas.id.clone().unwrap()));
    }
}

#[tokio::test]
async fn test_gwasinfo_empty_ids() {
    let Some(client) = require_client() else {
        return;
    };

    let result = client
        .gwasinfo(&GwasInfoRequest { id: vec![] })
        .await
        .unwrap();
    assert!(result.is_empty());
}

#[tokio::test]
async fn test_gwasinfo_nonexistent_id() {
    let Some(client) = require_client() else {
        return;
    };

    let result = client
        .gwasinfo(&GwasInfoRequest {
            id: vec!["nonexistent-id-99999".into()],
        })
        .await
        .unwrap();
    assert!(result.is_empty(), "nonexistent ID should return empty");
}

#[tokio::test]
async fn test_gwasinfo_refresh() {
    let Some(client) = require_client() else {
        return;
    };

    let all = client.gwasinfo_all().await.unwrap();
    let count_before = all.len();

    let refreshed = client.gwasinfo_refresh().await.unwrap();
    assert!(!refreshed.is_empty(), "refreshed data should not be empty");
    // After refresh, count should be the same (or updated if catalog changed).
    let count_after = client.gwasinfo_count().await.unwrap();
    println!("Before: {count_before}, After: {count_after}");
}

// ---------------------------------------------------------------------------
// GwasInfo files (auth required, always remote)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_gwasinfo_files() {
    let Some(client) = require_client() else {
        return;
    };

    let all = client.gwasinfo_all().await.unwrap();
    let known_id = all[0].id.clone().unwrap();

    let resp = client
        .gwasinfo_files(&GwasInfoFilesRequest {
            id: vec![known_id],
            commercial_approval_received: None,
        })
        .await
        .unwrap();
    assert!(resp.is_object() || resp.is_array());
}

// ---------------------------------------------------------------------------
// Associations
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_associations() {
    let Some(client) = require_client() else {
        return;
    };

    // Use a well-known variant and dataset.
    let resp = client
        .associations(&AssociationsRequest {
            variant: vec!["rs1205".into()],
            id: vec!["ukb-b-19953".into()],
            proxies: None,
            population: None,
            r2: None,
            align_alleles: None,
            palindromes: None,
            maf_threshold: None,
            commercial_approval_received: None,
        })
        .await
        .unwrap();
    assert!(resp.is_array() || resp.is_object());
    println!(
        "associations response: {}",
        serde_json::to_string_pretty(&resp).unwrap()
    );
}

// ---------------------------------------------------------------------------
// Tophits
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_tophits() {
    let Some(client) = require_client() else {
        return;
    };

    let resp = client
        .tophits(&TophitsRequest {
            id: vec!["ukb-b-19953".into()],
            pval: Some(1e-5),
            preclumped: None,
            clump: Some(0),
            r2: None,
            kb: None,
            pop: None,
            commercial_approval_received: None,
        })
        .await
        .unwrap();
    assert!(resp.is_array() || resp.is_object());
}

// ---------------------------------------------------------------------------
// PheWAS
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_phewas() {
    let Some(client) = require_client() else {
        return;
    };

    let resp = client
        .phewas(&PhewasRequest {
            variant: vec!["rs1205".into()],
            pval: Some(1e-10),
            index_list: None,
            commercial_approval_received: None,
        })
        .await
        .unwrap();
    assert!(resp.is_array() || resp.is_object());
}

// ---------------------------------------------------------------------------
// Variants
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_variants_rsid() {
    let Some(client) = require_client() else {
        return;
    };

    let resp = client
        .variants_rsid(&VariantsRsidRequest {
            rsid: vec!["rs1205".into()],
        })
        .await
        .unwrap();
    assert!(resp.is_object() || resp.is_array());
}

#[tokio::test]
async fn test_variants_chrpos() {
    let Some(client) = require_client() else {
        return;
    };

    let resp = client
        .variants_chrpos(&VariantsChrposRequest {
            chrpos: vec!["7:105561135".into()],
            radius: None,
        })
        .await
        .unwrap();
    assert!(resp.is_object() || resp.is_array());
}

#[tokio::test]
async fn test_variants_gene() {
    let Some(client) = require_client() else {
        return;
    };

    let resp = client.variants_gene("ENSG00000123374", None).await.unwrap();
    assert!(resp.is_object() || resp.is_array());
}

#[tokio::test]
async fn test_variants_afl2() {
    let Some(client) = require_client() else {
        return;
    };

    let resp = client
        .variants_afl2(&VariantsAfl2Request {
            rsid: vec!["rs1205".into()],
            chrpos: vec![],
            radius: None,
        })
        .await
        .unwrap();
    assert!(resp.is_object() || resp.is_array());
}

#[tokio::test]
async fn test_variants_afl2_snplist() {
    let Some(client) = require_client() else {
        return;
    };

    let resp = client.variants_afl2_snplist().await.unwrap();
    assert!(resp.is_object() || resp.is_array());
}

// ---------------------------------------------------------------------------
// LD
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_ld_clump() {
    let Some(client) = require_client() else {
        return;
    };

    let resp = client
        .ld_clump(&LdClumpRequest {
            rsid: vec!["rs1205".into()],
            pval: vec![5e-8],
            pthresh: None,
            r2: None,
            kb: None,
            pop: None,
        })
        .await
        .unwrap();
    assert!(resp.is_object() || resp.is_array());
}

#[tokio::test]
async fn test_ld_matrix() {
    let Some(client) = require_client() else {
        return;
    };

    let resp = client
        .ld_matrix(&LdMatrixRequest {
            rsid: vec!["rs1205".into(), "rs234".into()],
            pop: None,
        })
        .await
        .unwrap();
    assert!(resp.is_object() || resp.is_array());
}

#[tokio::test]
async fn test_ld_reflookup() {
    let Some(client) = require_client() else {
        return;
    };

    let resp = client
        .ld_reflookup(&LdReflookupRequest {
            rsid: vec!["rs1205".into()],
            pop: None,
        })
        .await
        .unwrap();
    assert!(resp.is_object() || resp.is_array());
}

// ---------------------------------------------------------------------------
// Edit — metadata (auth required)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_edit_list() {
    let Some(client) = require_client() else {
        return;
    };

    let resp = client
        .edit_list(&EditListQuery {
            state: Some("draft".into()),
            offset: None,
            limit: None,
        })
        .await;
    match resp {
        Ok(r) => {
            // May be empty if user has no drafts — just verify valid JSON.
            assert!(r.is_array() || r.is_object());
        }
        Err(e) => {
            // 401 means the token lacks edit/QC admin permissions.
            if e.to_string().contains("401") {
                println!("edit_list: 401 — token lacks edit permissions, skipping");
                return;
            }
            panic!("edit_list failed: {e}");
        }
    }
}

// ---------------------------------------------------------------------------
// Quality Control (auth required, admin-level)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_qc_list() {
    let Some(client) = require_client() else {
        return;
    };

    let resp = client.qc_list().await;
    match resp {
        Ok(r) => {
            assert!(r.is_array() || r.is_object());
        }
        Err(e) => {
            // 401 means the token lacks QC admin permissions.
            if e.to_string().contains("401") {
                println!("qc_list: 401 — token lacks QC permissions, skipping");
                return;
            }
            panic!("qc_list failed: {e}");
        }
    }
}
