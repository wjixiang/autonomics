//! VCF driver.

use std::sync::Arc;

use arrow_schema::SchemaRef;
use datafusion::error::Result;
use oxbow::variant::VcfScanner;
use oxbow::{CoordSystem, Select};

use super::super::core::{BioBatchIter, BioDriver, BioInput, buf_reader, map_ext};

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

impl BioDriver for VcfDriver {
    const FILE_TYPE: &'static str = "vcf";

    fn infer_schema(input: &BioInput) -> Result<SchemaRef> {
        let mut reader = noodles::vcf::io::Reader::new(buf_reader(input.bytes.clone(), input.gz));
        let header = reader.read_header().map_err(map_ext)?;
        let scanner = scanner(header)?;
        Ok(Arc::new(scanner.schema().clone()))
    }

    fn scan(input: BioInput, batch_size: usize) -> Result<BioBatchIter> {
        let mut reader = noodles::vcf::io::Reader::new(buf_reader(input.bytes, input.gz));
        let header = reader.read_header().map_err(map_ext)?;
        let scanner = scanner(header)?;
        let batches = scanner
            .scan(reader, None, Some(batch_size), None)
            .map_err(map_ext)?;
        Ok(Box::new(batches))
    }
}

#[cfg(test)]
mod tests {
    use arrow::array::Int64Array;
    use arrow_schema::ArrowError;
    use datafusion::catalog::MemTable;
    use datafusion::prelude::{SessionContext, col, lit};

    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};

    /// Path to the bundled `sample.vcf.gz` fixture (workspace root).
    fn fixture_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample.vcf.gz")
    }

    /// Read a gzipped VCF from `path` into a [`BioInput`], setting `gz: true`
    /// so the driver decompresses on the fly.
    fn load_vcf_gz_input(path: impl AsRef<Path>) -> BioInput {
        let path = path.as_ref();
        let raw = fs::read(path).expect("failed to read VCF.gz");

        println!("file: {}", path.display());
        println!("compressed size: {} bytes", raw.len());

        BioInput {
            gz: true,
            bytes: bytes::Bytes::from(raw),
        }
    }

    /// Parse the fixture VCF.gz file end-to-end and verify row/batch counts.
    ///
    /// Run with: cargo test -p biofusion -- vcf::tests::parse_vcf_gz --nocapture
    #[test]
    fn parse_vcf_gz() {
        let input = load_vcf_gz_input(fixture_path());

        // --- schema inference ---
        let schema = VcfDriver::infer_schema(&input).expect("infer_schema failed");
        println!("schema: {} columns", schema.fields().len());
        assert!(!schema.fields().is_empty(), "schema should have columns");

        // --- full scan ---
        let batch_size = 8192;
        let batches = VcfDriver::scan(input, batch_size).expect("scan failed");
        let mut total_rows = 0usize;
        let mut total_batches = 0usize;
        for batch in batches {
            let batch = batch.expect("batch error");
            total_rows += batch.num_rows();
            total_batches += 1;
        }

        println!("scan: {total_batches} batches, {total_rows} rows");
        assert!(total_rows > 0, "VCF.gz should contain rows");
        assert!(total_batches > 0, "should produce at least one batch");
    }

    #[tokio::test]
    async fn sql_query_vcf() {
        let input = load_vcf_gz_input("../../fixtures/opengwas_gwas_sumstat_sample.vcf.gz");
        // --- full scan ---
        let batch_size = 8192;
        let batches = VcfDriver::scan(input, batch_size)
            .expect("scan failed")
            .collect::<Result<Vec<_>, ArrowError>>()
            .unwrap();

        let ctx = SessionContext::new();
        let schema = batches[0].schema();
        let provider = Arc::new(MemTable::try_new(schema, vec![batches]).unwrap());
        ctx.register_table("vcf", provider).unwrap();

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
        // panic!()
    }
}
