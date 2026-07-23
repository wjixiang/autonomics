//! Sink the 1000G EUR LD matrix (zstd-compressed TSV) into Iceberg.
//!
//! One table per chromosome under the `ld_matrix` namespace, e.g.
//! `iceberg.ld_matrix.eur_chr22`. Each chromosome is written independently, so
//! the whole batch fans out across all CPU cores: up to
//! `available_parallelism()` chromosomes are sunk concurrently, each driving a
//! single-threaded zstd-decode → parquet-encode pipeline.
//!
//! Tables that already exist are skipped (idempotent re-runs). Drop a table
//! manually to force a rewrite.
//!
//! Run:
//!     cargo run -p sink_ld_matrix

use std::sync::Arc;

use datafusion::arrow::datatypes::{DataType, Field, Schema};
use datafusion::catalog::CatalogProvider;
use datafusion::datasource::file_format::file_compression_type::FileCompressionType;
use datafusion::prelude::{CsvReadOptions, SessionContext};
use datalake::Datalake;
use iceberg::arrow::arrow_schema_to_schema_auto_assign_ids;
use iceberg::{Catalog, NamespaceIdent, TableCreation, TableIdent};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

const LD_NAMESPACE: &str = "ld_matrix";
const LD_MATRIX_DATA_PATH: &str = "/mnt/disk2/dataset/1000g_plink/eur/ld/";

/// Target schema, in source-TSV column order.
///
/// Assigned *positionally* via `CsvReadOptions::schema`, which sidesteps
/// DataFusion name resolution: the TSV's first header is `#CHROM_A`, and
/// DataFusion treats `#` as a relation/column qualifier separator, so
/// `with_column_renamed`/`col` on it silently fail. The `#` is also rejected
/// by the Iceberg field-name spec, so we normalize to lowercase here.
/// `pos_a`/`pos_b` are safe — iceberg-rust reserves only the *bare* `pos`.
const TABLE_FIELDS: &[(&str, DataType)] = &[
    ("chrom_a", DataType::Int64),
    ("pos_a", DataType::Int64),
    ("id_a", DataType::Utf8),
    ("chrom_b", DataType::Int64),
    ("pos_b", DataType::Int64),
    ("id_b", DataType::Utf8),
    ("unphased_r2", DataType::Float64),
];

type AnyError = Box<dyn std::error::Error + Send + Sync>;

/// Build the target Arrow schema (column names assigned positionally).
fn target_schema() -> Schema {
    Schema::new(
        TABLE_FIELDS
            .iter()
            .map(|(name, dt)| Field::new(*name, dt.clone(), true))
            .collect::<Vec<_>>(),
    )
}

/// Outcome of sinking one chromosome, for the final summary.
enum SinkResult {
    Written,
    Skipped,
    Failed(String),
}

/// Sink a single chromosome file into `iceberg.ld_matrix.<table>`.
///
/// Skips silently if the table already exists. Each call owns its own
/// `SessionContext` so concurrent calls never share planner state; the
/// `Datalake` (catalog) handle is shared via `Arc`.
async fn sink_chromosome(
    datalake: Arc<Datalake>,
    namespace: NamespaceIdent,
    file_path: String,
    table_name: String,
) -> Result<(String, f64), AnyError> {
    let arrow_schema = target_schema();

    // Each chromosome gets its own context; the iceberg catalog is shared.
    let ctx = SessionContext::new();
    let opts = CsvReadOptions::new()
        .has_header(true)
        .schema(&arrow_schema)
        .delimiter(b'\t')
        .file_extension("zst")
        .file_compression_type(FileCompressionType::ZSTD);
    let df = ctx.read_csv(&file_path, opts).await?;

    // Skip only if the table already has committed data. A table can exist
    // but be empty if a prior run was killed mid-INSERT — Iceberg commits a
    // single snapshot only at the *end* of the INSERT, so a killed run leaves
    // an empty table. For those we fall through and populate them.
    let catalog = datalake.get_catalog().await?;
    let table_ident = TableIdent::new(namespace.clone(), table_name.clone());
    if catalog.table_exists(&table_ident).await? {
        let existing = catalog.load_table(&table_ident).await?;
        if existing.metadata().current_snapshot().is_some() {
            return Ok((table_name.clone(), 0.0));
        }
    }

    let started = std::time::Instant::now();

    // Create the table from the target schema.
    let iceberg_schema = arrow_schema_to_schema_auto_assign_ids(&arrow_schema)?;
    let creation = TableCreation::builder()
        .name(table_name.clone())
        .schema(iceberg_schema)
        .build();
    datalake
        .create_table_if_not_exist(&namespace, creation)
        .await?;

    // Refresh the provider so the planner sees the just-created table.
    let fresh_provider = datalake.get_provider().await?;
    ctx.register_catalog(
        "iceberg",
        Arc::new(fresh_provider) as Arc<dyn CatalogProvider>,
    );

    // Register the source as a temp view and INSERT.
    let src_name = format!("__sink_src_{:x}_{table_name}", std::process::id());
    ctx.register_table(&src_name, df.into_view())?;
    let fqn = format!("iceberg.{LD_NAMESPACE}.{table_name}");
    let sql = format!("INSERT INTO {fqn} SELECT * FROM {src_name}");
    ctx.sql(&sql).await?.collect().await?;
    ctx.deregister_table(&src_name)?;

    Ok((table_name, started.elapsed().as_secs_f64()))
}

/// Discover `1000G.EUR.chr<N>.ld.vcor.zst` files and their table names.
fn discover_chromosomes(dir: &str) -> Vec<(String, String)> {
    let prefix = "1000G.EUR.chr";
    let suffix = ".ld.vcor.zst";
    let mut out: Vec<(u32, String, String)> = std::fs::read_dir(dir)
        .expect("read LD data dir")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter_map(|name| {
            let n_str = name.strip_prefix(prefix)?.strip_suffix(suffix)?;
            let n: u32 = n_str.parse().ok()?;
            Some((n, name, format!("eur_chr{n}")))
        })
        .collect();
    out.sort_by_key(|(n, _, _)| *n);
    out.into_iter()
        .map(|(_, name, table)| (format!("{dir}{name}"), table))
        .collect()
}

#[tokio::main]
async fn main() -> Result<(), AnyError> {
    let datalake = Arc::new(Datalake::new());
    let namespace = NamespaceIdent::from_vec(vec![LD_NAMESPACE.to_string()])?;

    let chromosomes = discover_chromosomes(LD_MATRIX_DATA_PATH);
    if chromosomes.is_empty() {
        return Err(format!("no .zst chromosomes found in {LD_MATRIX_DATA_PATH}").into());
    }

    // Concurrency: default to one writer per core. Override with
    // `SINK_CONCURRENCY=N` (e.g. 2 to be kind to the shared spinning disk).
    let parallelism = std::env::var("SINK_CONCURRENCY")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4)
        });
    let sem = Arc::new(Semaphore::new(parallelism));
    println!(
        "sinking {} chromosomes, concurrency={parallelism}",
        chromosomes.len()
    );

    let mut tasks: JoinSet<SinkResult> = JoinSet::new();
    for (file_path, table_name) in chromosomes {
        let datalake = datalake.clone();
        let namespace = namespace.clone();
        let sem = sem.clone();
        tasks.spawn(async move {
            // Wait for a core slot before starting the heavy decode/encode.
            let _permit = match sem.acquire_owned().await {
                Ok(p) => p,
                Err(e) => {
                    return SinkResult::Failed(format!("{table_name}: semaphore closed: {e}"));
                }
            };
            println!("[start] {table_name}");
            match sink_chromosome(datalake, namespace, file_path, table_name.clone()).await {
                Ok((table, 0.0)) => {
                    println!("[skip ] {table} (exists)");
                    SinkResult::Skipped
                }
                Ok((table, secs)) => {
                    println!("[done ] {table} in {secs:.1}s");
                    SinkResult::Written
                }
                Err(e) => {
                    let msg = format!("{e}");
                    eprintln!("[FAIL ] {table_name}: {msg}");
                    SinkResult::Failed(format!("{table_name}: {msg}"))
                }
            }
        });
    }

    let mut written = 0usize;
    let mut skipped = 0usize;
    let mut failed = Vec::new();
    while let Some(res) = tasks.join_next().await {
        match res.expect("task panicked") {
            SinkResult::Written => written += 1,
            SinkResult::Skipped => skipped += 1,
            SinkResult::Failed(msg) => failed.push(msg),
        }
    }

    println!(
        "\nsummary: {written} written, {skipped} skipped, {} failed",
        failed.len()
    );
    for msg in &failed {
        println!("  - {msg}");
    }
    if !failed.is_empty() {
        return Err(format!("{} chromosome(s) failed", failed.len()).into());
    }
    Ok(())
}
