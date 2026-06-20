use gwascatalog_sdk::{AssociationQuery, ChromosomeAssociationQuery, GwasCatalogApi, PaginationQuery, RevealMode};

fn api() -> GwasCatalogApi {
    GwasCatalogApi::new()
}

// ── Associations ──────────────────────────────────────────────────────────

#[tokio::test]
async fn list_associations_default() {
    let resp = api()
        .list_associations(&AssociationQuery::default())
        .await
        .unwrap();
    assert!(resp._links.next.is_some());
    assert!(resp._links.first.is_some());
    assert!(resp._links.self_.is_some());
}

#[tokio::test]
async fn list_associations_with_paging() {
    let q = AssociationQuery {
        start: Some(0),
        size: Some(2),
        ..Default::default()
    };
    let resp = api().list_associations(&q).await.unwrap();
    let assoc = &resp._embedded.as_ref().unwrap().associations;
    assert!(assoc.len() <= 2);
}

#[tokio::test]
async fn list_associations_p_value_filter() {
    let q = AssociationQuery {
        p_lower: Some(1e-9),
        p_upper: Some(1e-8),
        ..Default::default()
    };
    let resp = api().list_associations(&q).await.unwrap();
    let assoc = &resp._embedded.as_ref().unwrap().associations;
    for a in assoc.values() {
        assert!(a.p_value >= 1e-9 && a.p_value <= 1e-8);
    }
}

#[tokio::test]
async fn list_associations_study_accession_filter() {
    let q = AssociationQuery {
        study_accession: Some("GCST000028".to_string()),
        ..Default::default()
    };
    let resp = api().list_associations(&q).await.unwrap();
    let assoc = &resp._embedded.as_ref().unwrap().associations;
    for a in assoc.values() {
        assert_eq!(a.study_accession, "GCST000028");
    }
}

#[tokio::test]
async fn list_associations_reveal_raw() {
    let q = AssociationQuery {
        reveal: Some(RevealMode::Raw),
        size: Some(1),
        ..Default::default()
    };
    let resp = api().list_associations(&q).await.unwrap();
    assert!(resp._embedded.is_some());
}

// ── Variant associations ──────────────────────────────────────────────────

#[tokio::test]
async fn get_variant_associations() {
    let q = AssociationQuery {
        size: Some(2),
        ..Default::default()
    };
    let resp = api()
        .get_variant_associations("rs7903146", &q)
        .await
        .unwrap();
    assert!(resp._embedded.is_some());
    let assoc = &resp._embedded.as_ref().unwrap().associations;
    for a in assoc.values() {
        assert_eq!(a.variant_id, "rs7903146");
    }
}

// ── Chromosomes ───────────────────────────────────────────────────────────

#[tokio::test]
async fn list_chromosomes_default() {
    let resp = api()
        .list_chromosomes(&PaginationQuery::default())
        .await
        .unwrap();
    assert!(resp._embedded.is_some());
    let chroms = &resp._embedded.as_ref().unwrap().chromosomes;
    assert!(!chroms.is_empty());
}

#[tokio::test]
async fn list_chromosomes_with_paging() {
    let q = PaginationQuery {
        start: Some(0),
        size: Some(5),
    };
    let resp = api().list_chromosomes(&q).await.unwrap();
    let chroms = &resp._embedded.as_ref().unwrap().chromosomes;
    assert!(!chroms.is_empty());
}

#[tokio::test]
async fn get_chromosome() {
    let chrom = api().get_chromosome("1").await.unwrap();
    assert_eq!(chrom.chromosome, "1");
}

// ── Chromosome associations ────────────────────────────────────────────────

#[tokio::test]
async fn list_chromosome_associations_default() {
    let q = ChromosomeAssociationQuery {
        size: Some(2),
        ..Default::default()
    };
    let resp = api().list_chromosome_associations("1", &q).await.unwrap();
    assert!(resp._embedded.is_some());
    let assoc = &resp._embedded.as_ref().unwrap().associations;
    for a in assoc.values() {
        assert_eq!(a.chromosome, 1);
    }
}

#[tokio::test]
async fn list_chromosome_associations_bp_filter() {
    let q = ChromosomeAssociationQuery {
        bp_lower: Some(1000000),
        bp_upper: Some(2000000),
        size: Some(5),
        ..Default::default()
    };
    let resp = api().list_chromosome_associations("1", &q).await.unwrap();
    let assoc = &resp._embedded.as_ref().unwrap().associations;
    for a in assoc.values() {
        assert!(
            a.base_pair_location >= 1000000 && a.base_pair_location <= 2000000
        );
    }
}

#[tokio::test]
async fn get_variant_on_chromosome() {
    let q = AssociationQuery {
        size: Some(2),
        ..Default::default()
    };
    let resp = api()
        .get_variant_on_chromosome("10", "rs7903146", &q)
        .await
        .unwrap();
    assert!(resp._embedded.is_some());
    let assoc = &resp._embedded.as_ref().unwrap().associations;
    for a in assoc.values() {
        assert_eq!(a.chromosome, 10);
        assert_eq!(a.variant_id, "rs7903146");
    }
}

// ── Traits ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn list_traits_default() {
    let resp = api()
        .list_traits(&PaginationQuery::default())
        .await
        .unwrap();
    assert!(resp._links.self_.is_some());
}

#[tokio::test]
async fn list_traits_with_paging() {
    let q = PaginationQuery {
        start: Some(0),
        size: Some(3),
    };
    let resp = api().list_traits(&q).await.unwrap();
    let traits = &resp._embedded.as_ref().unwrap().traits;
    assert!(traits.len() <= 3);
}

#[tokio::test]
async fn get_trait() {
    let trait_id = "EFO_0001360";
    let t = api().get_trait(trait_id).await.unwrap();
    assert_eq!(t.trait_, trait_id);
}

#[tokio::test]
async fn list_trait_associations() {
    let q = AssociationQuery {
        size: Some(2),
        ..Default::default()
    };
    let resp = api()
        .list_trait_associations("EFO_0001360", &q)
        .await
        .unwrap();
    assert!(resp._embedded.is_some());
}

#[tokio::test]
async fn list_trait_studies() {
    let q = PaginationQuery {
        size: Some(3),
        ..Default::default()
    };
    let resp = api()
        .list_trait_studies("EFO_0001360", &q)
        .await
        .unwrap();
    assert!(resp._embedded.is_some());
}

// ── Studies ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn list_studies_default() {
    let resp = api()
        .list_studies(&PaginationQuery::default())
        .await
        .unwrap();
    assert!(resp._links.self_.is_some());
}

#[tokio::test]
async fn list_studies_with_paging() {
    let q = PaginationQuery {
        start: Some(0),
        size: Some(2),
    };
    let resp = api().list_studies(&q).await.unwrap();
    assert!(resp._embedded.is_some());
}

#[tokio::test]
async fn get_study() {
    let s = api().get_study("GCST000028").await.unwrap();
    assert_eq!(s.study_accession, "GCST000028");
}

#[tokio::test]
async fn list_study_associations() {
    let q = AssociationQuery {
        size: Some(2),
        ..Default::default()
    };
    let resp = api()
        .list_study_associations("GCST000028", &q)
        .await
        .unwrap();
    assert!(resp._embedded.is_some());
    let assoc = &resp._embedded.as_ref().unwrap().associations;
    for a in assoc.values() {
        assert_eq!(a.study_accession, "GCST000028");
    }
}

// ── Error handling ────────────────────────────────────────────────────────

#[tokio::test]
async fn get_nonexistent_trait_returns_api_error() {
    let err = api()
        .get_trait("EFO_NONEXISTENT")
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        gwascatalog_sdk::gwas_catalog::GwasCatalogError::Api { .. }
    ));
}

#[tokio::test]
async fn get_nonexistent_study_returns_error() {
    let err = api()
        .get_study("GCST999999")
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        gwascatalog_sdk::gwas_catalog::GwasCatalogError::Api { .. }
    ));
}

#[tokio::test]
async fn get_nonexistent_trait_returns_error() {
    let err = api()
        .get_trait("EFO_NONEXISTENT")
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        gwascatalog_sdk::gwas_catalog::GwasCatalogError::Api { .. }
    ));
}

// ── Data structure validation ─────────────────────────────────────────────

#[tokio::test]
async fn association_fields_populated() {
    let q = AssociationQuery {
        size: Some(1),
        reveal: Some(RevealMode::All),
        ..Default::default()
    };
    let resp = api().list_associations(&q).await.unwrap();
    let assoc = &resp._embedded.as_ref().unwrap().associations;
    let a = assoc.values().next().unwrap();
    assert!(!a.variant_id.is_empty());
    assert!(a.chromosome >= 1 && a.chromosome <= 22);
    assert!(a.base_pair_location > 0);
    assert!(!a.study_accession.is_empty());
    assert!(!a.trait_.is_empty());
    assert!(a.p_value > 0.0);
}
