//! VCF driver.
//!
//! This is the first format ported to oxbow's async scanner, so — unlike the
//! other drivers — it **streams** the object from the store instead of fetching
//! it whole: the scan path never materializes the file, and a downstream limit
//! that drops the stream stops fetching from the store.

use std::sync::Arc;

use arrow_schema::{ArrowError, SchemaRef};
use async_trait::async_trait;
use datafusion::error::Result;
use datafusion::object_store::{ObjectStore, ObjectStoreExt};
use futures::{StreamExt, TryStreamExt};
use oxbow::async_scanner::AsyncScanner;
use oxbow::variant::VcfScanner;
use oxbow::{CoordSystem, Select};
use tokio::io::AsyncBufRead;

use super::super::core::{BioBatchStream, BioDriver, BioInput, map_ext};

/// Build an oxbow [`VcfScanner`] selecting every contig / field / sample
/// (full schema). Shared by schema inference and the runtime scan so they
/// agree on the produced schema.
fn scanner(header: noodles::vcf::Header) -> Result<VcfScanner> {
    VcfScanner::new(
        header,
        Select::All,
        Select::All,
        Select::All,
        None,
        Select::All,
        None,
        CoordSystem::OneClosed,
    )
    .map_err(map_ext)
}

pub struct VcfDriver;

/// Open a streaming, decompressed async reader over the object.
///
/// Bytes are pulled from the object store on demand (never the whole object at
/// once) and gzip/BGZF is decoded as a multi-member stream when `gz` is set.
/// Dropping the returned reader — e.g. when a downstream limit is reached —
/// stops the store from being polled further.
async fn open_reader(
    input: &BioInput,
) -> Result<Box<dyn AsyncBufRead + Unpin + Send>> {
    use async_compression::tokio::bufread::GzipDecoder;
    use tokio_util::io::StreamReader;

    // `into_stream()` yields chunks lazily; `StreamReader` adapts the chunk
    // stream into an `AsyncRead` backed by the store's HTTP body.
    let chunk_stream = input
        .store
        .get(&input.location)
        .await?
        .into_stream()
        .map_err(std::io::Error::other);
    let raw = tokio::io::BufReader::new(StreamReader::new(chunk_stream));
    if input.gz {
        let mut decoder = GzipDecoder::new(raw);
        // BGZF is a stream of concatenated gzip members; decode them all.
        decoder.multiple_members(true);
        // GzipDecoder is AsyncRead-only, so wrap it once more for AsyncBufRead.
        Ok(Box::new(tokio::io::BufReader::new(decoder)))
    } else {
        Ok(Box::new(raw))
    }
}

#[async_trait]
impl BioDriver for VcfDriver {
    const FILE_TYPE: &'static str = "vcf";

    async fn infer_schema(input: &BioInput) -> Result<SchemaRef> {
        // Only the header is needed for the schema, so stream just enough to
        // read it and drop the reader.
        let reader = open_reader(input).await?;
        let mut vcf_reader = noodles::vcf::r#async::io::Reader::new(reader);
        let header = vcf_reader.read_header().await.map_err(map_ext)?;
        let scanner = scanner(header)?;
        Ok(Arc::new(scanner.schema().clone()))
    }

    async fn scan(
        input: BioInput,
        batch_size: usize,
        limit: Option<usize>,
    ) -> Result<BioBatchStream> {
        // Read the header from a first stream (only header bytes transferred)
        // to build the scanner — the same path as schema inference.
        let header_reader = open_reader(&input).await?;
        let mut vcf_reader = noodles::vcf::r#async::io::Reader::new(header_reader);
        let header = vcf_reader.read_header().await.map_err(map_ext)?;
        let scanner = scanner(header)?;

        // Scan records from a fresh stream from byte 0. The async scanner skips
        // the embedded header itself; dropping the stream on a limit stops
        // fetching from the store.
        let records_reader = open_reader(&input).await?;
        let stream = AsyncScanner::scan(&scanner, records_reader, Some(batch_size), limit)
            .map(|res| res.map_err(|e| ArrowError::from_external_error(Box::new(e))));
        Ok(Box::pin(stream))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use arrow::array::Int64Array;
    use datafusion::catalog::MemTable;
    use datafusion::prelude::{SessionContext, col, lit};

    use crate::datasource::BioReadOptions;
    use crate::ext::DataFusionReadExt;

    /// Path to the bundled `sample.vcf.gz` fixture (workspace root).
    fn fixture_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample.vcf.gz")
    }

    /// Path to the opengwas sample fixture used for the SQL/struct-field regression.
    fn opengwas_fixture_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/opengwas_gwas_sumstat_sample.vcf.gz")
    }

    /// Drive the VCF scan through the full DataFusion stack (which now streams
    /// from the object store on the scan path) and verify it produces rows.
    ///
    /// Run with: cargo test -p biofusion -- vcf::tests::parse_vcf_gz --nocapture
    #[tokio::test]
    async fn parse_vcf_gz() {
        let path = fixture_path();
        println!("file: {}", path.display());

        // --- schema inference (also streaming — only the header is read) ---
        let ctx = SessionContext::new();
        let df = ctx
            .read_vcf(path.to_str().unwrap(), BioReadOptions::default())
            .await
            .expect("read_vcf failed");
        let schema = df.schema();
        println!("schema: {} columns", schema.fields().len());
        assert!(!schema.fields().is_empty(), "schema should have columns");

        // --- streaming scan + early decode stop ---
        let batches = df.collect().await.expect("collect failed");
        let total_rows: usize = batches.iter().map(|b| b.num_rows()).sum();
        let total_batches = batches.len();
        println!("scan: {total_batches} batches, {total_rows} rows");
        assert!(total_rows > 0, "VCF.gz should contain rows");
        assert!(total_batches > 0, "should produce at least one batch");
    }

    #[tokio::test]
    async fn sql_query_vcf() {
        let path = opengwas_fixture_path();

        // Collect the streaming scan into an in-memory table so we can run SQL
        // (struct-field queries, GROUP BY, LIMIT) on top of the streamed batches.
        let ctx = SessionContext::new();
        let df = ctx
            .read_vcf(path.to_str().unwrap(), BioReadOptions::default())
            .await
            .expect("read_vcf failed");
        let batches = df.collect().await.expect("collect failed");
        let schema = batches[0].schema();
        let provider = MemTable::try_new(Arc::clone(&schema), vec![batches]).unwrap();
        ctx.register_table("vcf", Arc::new(provider)).unwrap();

        // --- inspect schema (handy when picking columns to query) ---
        println!("schema:");
        for field in ctx.table("vcf").await.expect("table vcf").schema().fields() {
            println!("  {} : {:?}", field.name(), field.data_type());
        }

        // --- 1. total row count via SQL ---
        let total_batches = ctx
            .sql("SELECT COUNT(*) AS n FROM vcf")
            .await
            .expect("count sql failed")
            .collect()
            .await
            .expect("count collect failed");
        let total_rows = total_batches[0]
            .column(0)
            .as_any()
            .downcast_ref::<Int64Array>()
            .expect("count column is Int64")
            .value(0) as usize;
        println!("total rows via SQL: {total_rows}");
        assert!(total_rows > 0, "VCF should have rows");

        // --- 2. variants per chromosome (GROUP BY aggregate) ---
        let by_chrom = ctx
            .sql("SELECT chrom, COUNT(*) AS n FROM vcf GROUP BY chrom ORDER BY chrom")
            .await
            .expect("group-by sql failed");
        by_chrom.show().await.expect("show group-by failed");

        // --- 3. project + filter + limit via the DataFrame API ---
        let preview = ctx
            .table("vcf")
            .await
            .expect("table vcf")
            .select_columns(&["chrom", "pos"])
            .expect("select chrom/pos")
            .filter(col("chrom").eq(lit("1")))
            .expect("filter chrom = 1")
            .limit(0, Some(5))
            .expect("limit 5");
        let shown = preview.collect().await.expect("preview collect");
        let shown_rows: usize = shown.iter().map(|b| b.num_rows()).sum();
        println!("{shown_rows} variants on chrom=1 (first 5)");
        assert!(shown_rows <= 5);

        // --- 4. query struct fields
        let sample_field = ctx
            .sql("SELECT \"ukb-e-250_CSA\".\"ES\" FROM vcf LIMIT 10")
            .await
            .unwrap();
        sample_field.show().await.unwrap();
    }
}
