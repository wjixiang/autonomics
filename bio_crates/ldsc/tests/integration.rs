//! End-to-end h² estimation on gwas-simulate fixtures.
//!
//! Gated on `LDSC_RUN_INTEGRATION=1` so CI without the fixtures skips. Generate
//! the fixtures with `bash tests/generate_fixtures.sh`, or point `LDSC_FIXTURE_DIR`
//! at an existing gwas-simulate output directory (default
//! `/mnt/disk3/gwas-simulate/output`).
//!
//! The fixture files have no `.csv` extension, so we parse the TSVs ourselves
//! with `arrow-csv` and register the batches, then join via SQL.

use std::env;
use std::fs::File;
use std::io::BufReader;
use std::sync::Arc;

use arrow::compute::concat_batches;
use arrow_array::RecordBatch;
use arrow_csv::ReaderBuilder;
use arrow_schema::{DataType, Field, Schema};
use datafusion::prelude::SessionContext;
use ldsc::hsq::{HsqColumns, estimate_h2};

fn fixture_dir() -> String {
    env::var("LDSC_FIXTURE_DIR").unwrap_or_else(|_| "/mnt/disk3/gwas-simulate/output".to_string())
}

/// Read a tab-separated file with header into a single RecordBatch.
fn read_tsv(path: &str, schema: Vec<(&str, DataType)>) -> RecordBatch {
    let fields: Vec<Field> = schema
        .iter()
        .map(|(name, ty)| Field::new(*name, ty.clone(), false))
        .collect();
    let schema = Arc::new(Schema::new(fields));
    let reader = ReaderBuilder::new(schema)
        .with_delimiter(b'\t')
        .with_header(true)
        .build(BufReader::new(
            File::open(path).unwrap_or_else(|e| panic!("open {path}: {e}")),
        ))
        .unwrap_or_else(|e| panic!("build reader for {path}: {e}"));
    let batches: Vec<RecordBatch> = reader
        .collect::<Result<_, _>>()
        .unwrap_or_else(|e| panic!("read {path}: {e}"));
    assert!(!batches.is_empty(), "no rows in {path}");
    let refs: Vec<&RecordBatch> = batches.iter().collect();
    concat_batches(&batches[0].schema(), refs).unwrap_or_else(|e| panic!("concat {path}: {e}"))
}

#[tokio::test]
async fn estimate_h2_on_gwas_simulate_fixture() {
    if env::var("LDSC_RUN_INTEGRATION").ok().as_deref() != Some("1") {
        eprintln!("skipping (set LDSC_RUN_INTEGRATION=1 to run)");
        return;
    }

    let dir = fixture_dir();
    let sumstats = format!("{dir}/sumstats/0");
    let ref_ld = format!("{dir}/ldscore/oneld_onefile.l2.ldscore");
    let w_ld = format!("{dir}/ldscore/w.l2.ldscore");
    let m_path = format!("{dir}/ldscore/oneld_onefile.l2.M_5_50");

    let m: f64 = std::fs::read_to_string(&m_path)
        .unwrap_or_else(|e| panic!("read {m_path}: {e}"))
        .trim()
        .parse()
        .unwrap_or_else(|e| panic!("parse M from {m_path}: {e}"));

    let ctx = SessionContext::new();
    let sumstats_schema = vec![
        ("SNP", DataType::Utf8),
        ("A1", DataType::Utf8),
        ("A2", DataType::Utf8),
        ("N", DataType::Float64),
        ("Z", DataType::Float64),
    ];
    let ld_schema = vec![
        ("CHR", DataType::Int64),
        ("SNP", DataType::Utf8),
        ("BP", DataType::Int64),
        ("LD", DataType::Float64),
    ];
    let _ = ctx.register_batch("sumstats", read_tsv(&sumstats, sumstats_schema));
    let _ = ctx.register_batch("ldscore", read_tsv(&ref_ld, ld_schema.clone()));
    let _ = ctx.register_batch("w_ldscore", read_tsv(&w_ld, ld_schema));

    // Join on SNP and order by BP so the block jackknife sees genomic order
    // (LDSC sorts the same way before regressing). Column names are quoted
    // because the fixtures use uppercase headers and the SQL parser lowercases
    // unquoted identifiers.
    let df = ctx
        .sql(
            "SELECT a.\"Z\" AS z, a.\"N\" AS n, b.\"LD\" AS ref_ld, c.\"LD\" AS w_ld \
             FROM sumstats a \
             JOIN ldscore b ON a.\"SNP\" = b.\"SNP\" \
             JOIN w_ldscore c ON a.\"SNP\" = c.\"SNP\" \
             ORDER BY b.\"BP\"",
        )
        .await
        .expect("build joined DataFrame");

    let cols = HsqColumns {
        snp: "snp",
        z: "z",
        n: "n",
        ref_ld: vec!["ref_ld"],
        w_ld: "w_ld",
    };
    let res = estimate_h2(df, cols, &[m], 200, None)
        .await
        .expect("estimate_h2");

    println!("{res:#?}");

    assert_eq!(res.n_snp, 1000, "expected 1000 SNPs");
    assert!(
        res.mean_chisq.is_finite() && res.mean_chisq > 1.0,
        "mean_chisq={}",
        res.mean_chisq
    );
    assert!(
        res.h2.is_finite() && res.h2 > 0.0 && res.h2 < 1.5,
        "h2 out of range: {}",
        res.h2
    );
    assert!(
        res.h2_se.is_finite() && res.h2_se > 0.0,
        "h2_se={}",
        res.h2_se
    );
    let intercept = res.intercept.expect("free intercept");
    assert!(
        intercept.is_finite() && (intercept - 1.0).abs() < 1.0,
        "intercept={intercept}"
    );
    assert!(res.lambda_gc.is_finite(), "lambda_gc={}", res.lambda_gc);
}
