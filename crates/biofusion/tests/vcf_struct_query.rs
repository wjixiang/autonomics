//! Regression for the agent-reported failure: querying a nested (struct)
//! field of a VCF sample column via SQL dot-notation through the
//! `read_vcf` → ListingTable path.
//!
//! Reproduces (in-process) the exact pipeline the data-engine agent drove:
//!
//! ```text
//!   add_source_node(format=vcf, path=opengwas_gwas_sumstat_sample.vcf.gz)
//!   add_sql_node(query = SELECT "ukb-e-250_CSA"."ES" AS es FROM port_0)
//!   get_output -> collect()
//! ```
//!
//! which currently fails at execution time with:
//!
//! ```text
//!   Execution error: get_field is only possible on maps or structs.
//!   Received Dictionary(Int32, Utf8) with Utf8("ES") index
//! ```
//!
//! ## Root cause (located via the diagnostics below)
//!
//! oxbow's `VcfScanner::scan` emits the `ukb-e-250_CSA` column as a
//! **Struct** at every batch size, under both the bgzf and flate2 decoders
//! (see diagnostic B). The Dictionary only appears **inside biofusion's
//! DataFusion scan plumbing, under column projection**:
//!
//!   - `SELECT *`              → column dtype is Struct   ✓ (unprojected)
//!   - `SELECT col`            → column dtype is Dictionary ✗ (projection)
//!   - `SELECT col.sub`        → get_field rejects the Dictionary ✗
//!
//! So the Struct→Dictionary conversion happens in `BioSource`'s use of
//! `ProjectionOpener` / `SplitProjection` (`crates/biofusion/src/datasource/
//! core.rs`, `create_file_opener`), NOT in oxbow or the decompression layer.
//!
//! This test pins the DESIRED behavior (struct-field query should plan AND
//! execute, returning rows). It is intentionally RED until fixed.

use std::io::Cursor;
use std::path::PathBuf;

use arrow::array::RecordBatchReader;
use biofusion::datasource::BioReadOptions;
use biofusion::datasource::core::{is_bgzf, is_gzip};
use biofusion::ext::DataFusionReadExt;
use datafusion::prelude::SessionContext;
use oxbow::{CoordSystem, Select};

/// Workspace-root fixture that triggers the failure (same file the agent used).
fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
}

/// Name of the VCF sample (FORMAT/genotype) column that carries the nested
/// struct the agent tried to project (`ES` effect-size subfield).
const SAMPLE_COL: &str = "ukb-e-250_CSA";
const SUBFIELD: &str = "ES";

/// Build the oxbow scanner with the same `Select` config biofusion uses, then
/// scan `bytes` (already wrapped in a decoder) and return the *executed* dtype
/// of `SAMPLE_COL` in the first produced batch. `batch_size` is passed through
/// to oxbow's `scan()` so we can test whether it changes the produced dtype.
fn scan_dtype<R: std::io::BufRead + 'static>(
    mut reader: noodles::vcf::io::Reader<R>,
    batch_size: Option<usize>,
) -> String {
    let header = reader.read_header().unwrap();
    let scanner = oxbow::variant::VcfScanner::new(
        header,
        Select::All,
        Select::All,
        Select::All,
        None,
        Select::All,
        None,
        CoordSystem::OneClosed,
    )
    .unwrap();
    let mut batches = scanner.scan(reader, None, batch_size, None).unwrap();
    let batch = batches.next().unwrap().unwrap();
    format!(
        "{:?} (batch_size={:?}, {} rows)",
        batch
            .schema()
            .field_with_name(SAMPLE_COL)
            .unwrap()
            .data_type(),
        batch_size,
        batch.num_rows()
    )
}

#[tokio::test]
async fn read_vcf_struct_field_query_succeeds() {
    let path = fixture("opengwas_gwas_sumstat_sample.vcf.gz");
    let raw = std::fs::read(&path).unwrap();

    // ── diagnostic A: which decompression path does biofusion pick? ────────
    // The first gzip member's FLG byte decides. is_bgzf requires FEXTRA(0x04)
    // + the `BC` subfield; this file's first member sets FNAME(0x08) only, so
    // is_bgzf()==false and biofusion falls back to flate2 MultiGzDecoder.
    eprintln!("[diag] is_gzip(raw) = {}", is_gzip(&raw));
    eprintln!("[diag] is_bgzf(raw) = {}", is_bgzf(&raw));
    eprintln!(
        "[diag] first gzip member FLG byte = 0x{:02x} (0x04=FEXTRA, 0x08=FNAME)",
        raw[3]
    );

    // ── diagnostic B: oxbow emits Struct regardless of decoder / batch_size ─
    // Rules out oxbow and the decompression layer: under every batch size and
    // both decoders, the produced column is Struct. The Dictionary seen via
    // read_vcf therefore originates downstream, in biofusion's scan plumbing.
    let dtype_bgzf = scan_dtype(
        noodles::vcf::io::Reader::new(noodles::bgzf::io::Reader::new(Cursor::new(raw.clone()))),
        None,
    );
    let dtype_flate2 = scan_dtype(
        noodles::vcf::io::Reader::new(std::io::BufReader::new(flate2::read::MultiGzDecoder::new(
            Cursor::new(raw.clone()),
        ))),
        None,
    );
    eprintln!("[diag] dtype via noodles bgzf reader : {dtype_bgzf}");
    eprintln!("[diag] dtype via flate2 MultiGzDecoder : {dtype_flate2}");
    eprintln!(
        "[diag] decoders agree                 : {}",
        dtype_bgzf == dtype_flate2
    );

    // ── diagnostic B2: does batch_size change the produced dtype? ──────────
    // biofusion's BioOpener passes batch_size=8192 (BioOptions::default);
    // the standalone scans above pass None (→ 1024). If the dtype flips at a
    // larger batch_size, that's an oxbow batch-builder bug.
    for bs in [None, Some(1024), Some(8192), Some(10000)] {
        let dt = scan_dtype(
            noodles::vcf::io::Reader::new(noodles::bgzf::io::Reader::new(Cursor::new(raw.clone()))),
            bs,
        );
        eprintln!("[diag] oxbow scan batch_size={bs:?} → {dt}");
    }

    // ── the read_vcf path (what the agent uses) ────────────────────────────
    let ctx = SessionContext::new();
    let df = ctx
        .read_vcf(path.to_str().unwrap(), BioReadOptions::default())
        .await
        .expect("read_vcf should succeed");

    let registered_type = df
        .schema()
        .field_with_name(None, SAMPLE_COL)
        .map(|f| f.data_type().clone())
        .expect("sample column must exist");
    eprintln!("[diag] registered/planner schema {SAMPLE_COL}: {registered_type:?}");

    ctx.register_table("vcf", df.into_view())
        .expect("register vcf view");

    // ── diagnostic C: executed dtype of the column INSIDE the read_vcf path ─
    // Collect the FULL table (SELECT *) so no projection/get_field runs, then
    // inspect the array dtype. oxbow's D::scan emits Struct; if this prints
    // Dictionary, the Struct→Dictionary conversion happens inside biofusion's
    // DataFusion scan plumbing (BioOpener stream → ProjectionOpener → collect),
    // NOT in oxbow or the decoder.
    let full = ctx
        .sql("SELECT * FROM vcf")
        .await
        .expect("select * should plan")
        .collect()
        .await;
    match full {
        Ok(batches) => {
            let idx = batches[0]
                .schema()
                .index_of(SAMPLE_COL)
                .expect("sample col index");
            eprintln!(
                "[diag] read_vcf SELECT * → {SAMPLE_COL} executed dtype: {:?}",
                batches[0].column(idx).data_type()
            );
        }
        Err(e) => eprintln!("[diag] read_vcf SELECT * FAILED: {e}"),
    }

    // Plain `SELECT col LIMIT 1` (no get_field) — same question, narrower.
    let raw_exec = ctx
        .sql(&format!(r#"SELECT "{SAMPLE_COL}" FROM vcf LIMIT 1"#))
        .await
        .expect("plain select should plan")
        .collect()
        .await;
    match raw_exec {
        Ok(b) => eprintln!(
            "[diag] read_vcf path, plain SELECT col → executed dtype: {:?}",
            b[0].column(0).data_type()
        ),
        Err(e) => eprintln!("[diag] read_vcf path, plain SELECT col FAILED: {e}"),
    }

    // ── the target query: project a nested struct subfield via dot-notation ─
    let query = format!(r#"SELECT "{SAMPLE_COL}"."{SUBFIELD}" AS es FROM vcf LIMIT 10"#);
    let planned = ctx.sql(&query).await.expect("struct-field SQL should plan");

    // This `.collect()` is where the agent's run blew up. It MUST succeed
    // and return at least one row once the underlying issue is fixed.
    let batches = planned
        .collect()
        .await
        .expect("struct-field query should execute, not reject get_field");
    let rows: usize = batches.iter().map(|b| b.num_rows()).sum();
    assert!(rows > 0, "struct-field query should return rows");
}

// #[tokio::test]
// async fn replicate_unnessary_list() {
//     let path = fixture("opengwas_gwas_sumstat_sample.vcf.gz");
//
//     let ctx = SessionContext::new();
//     let df = ctx
//         .read_vcf(path.to_str().unwrap(), BioReadOptions::default())
//         .await
//         .expect("read_vcf should succeed");
//
//     let schema = df.schema().inner().as_ref().clone();
//     dbg!(schema);
//     panic!()
// }
