pub mod error;
pub mod file_format;
pub mod source;

use std::sync::Arc;

use arrow_schema::SchemaRef;
use async_trait::async_trait;
use datafusion::common::Result;
use datafusion::config::TableOptions;
use datafusion::datasource::listing::{ListingOptions, ListingTableUrl};
use datafusion::execution::SessionState;
use datafusion::execution::options::ReadOptions;
use datafusion::prelude::*;

use crate::datasource_vcf::file_format::{VcfFormat, VcfOptions};

/// Options for reading VCF files.
///
/// Follows the same pattern as DataFusion's `CsvReadOptions`, `ParquetReadOptions`, etc.:
/// concrete type, not `impl ReadOptions<'a>`.
#[derive(Clone, Debug, Default)]
pub struct VcfReadOptions<'a> {
    file_extension: Option<String>,
    batch_size: Option<usize>,
    limit: Option<usize>,
    columns: Option<Vec<&'a str>>,
}

impl<'a> VcfReadOptions<'a> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_file_extension(mut self, ext: impl Into<String>) -> Self {
        self.file_extension = Some(ext.into());
        self
    }

    pub fn file_extension(&self) -> Option<&str> {
        self.file_extension.as_deref()
    }

    pub fn with_batch_size(mut self, batch_size: usize) -> Self {
        self.batch_size = Some(batch_size);
        self
    }

    pub fn batch_size(&self) -> Option<usize> {
        self.batch_size
    }

    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    pub fn limit(&self) -> Option<usize> {
        self.limit
    }

    pub fn with_columns(mut self, columns: Vec<&'a str>) -> Self {
        self.columns = Some(columns);
        self
    }

    pub fn columns(&self) -> Option<&[&'a str]> {
        self.columns.as_deref()
    }
}

#[async_trait]
impl<'a> ReadOptions<'a> for VcfReadOptions<'a> {
    #[doc = " Helper to convert these user facing options to `ListingTable` options"]
    fn to_listing_options(
        &self,
        _config: &SessionConfig,
        _table_options: TableOptions,
    ) -> ListingOptions {
        let mut vcf_options = VcfOptions::default();
        if let Some(batch_size) = self.batch_size {
            vcf_options.batch_size = batch_size;
        }
        let format = Arc::new(VcfFormat::new().with_options(vcf_options));
        ListingOptions::new(format)
            .with_file_extension(self.file_extension.clone().unwrap_or_else(|| "vcf".to_string()))
            .with_collect_stat(false)
    }

    #[doc = " Infer and resolve the schema from the files/sources provided."]
    #[allow(
        mismatched_lifetime_syntaxes,
        clippy::type_complexity,
        clippy::type_repetition_in_bounds
    )]
    async fn get_resolved_schema(
        &self,
        config: &SessionConfig,
        state: SessionState,
        table_path: ListingTableUrl,
    ) -> Result<SchemaRef> {
        self.to_listing_options(config, state.default_table_options())
            .infer_schema(&state, &table_path)
            .await
    }
}

#[cfg(test)]
mod tests {
    use datafusion::prelude::SessionContext;

    use crate::datasource_vcf::VcfReadOptions;
    use crate::ext::DataFusionReadExt;

    fn test_data_path(file: &str) -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_datasets")
            .join(file)
    }

    /// BGZF-compress `sample.vcf` into a `.vcf.gz` under the temp dir and return
    /// its path (covers the compressed-input path end-to-end).
    fn write_sample_vcf_gz() -> std::path::PathBuf {
        use std::io::Write;

        let raw = std::fs::read(test_data_path("sample.vcf")).unwrap();
        let mut enc = noodles::bgzf::io::Writer::new(Vec::new());
        enc.write_all(&raw).unwrap();
        enc.try_finish().unwrap();
        let bytes = enc.into_inner();

        let path = std::env::temp_dir().join("biofusion_sample.vcf.gz");
        std::fs::write(&path, &bytes).unwrap();
        path
    }

    // --- read_vcf ---

    #[tokio::test]
    async fn test_read_vcf_gz() {
        let path = write_sample_vcf_gz();
        let ctx = SessionContext::new();
        let df = ctx
            .read_vcf(path.to_str().unwrap(), VcfReadOptions::default())
            .await
            .unwrap();

        let batches = df.collect().await.unwrap();
        let row_count: usize = batches.iter().map(|b| b.num_rows()).sum();
        assert_eq!(row_count, 3, "expected 3 records from sample.vcf.gz");
    }

    #[tokio::test]
    async fn test_read_vcf_plain() {
        let ctx = SessionContext::new();
        let path = test_data_path("sample.vcf");
        let df = ctx
            .read_vcf(path.to_str().unwrap(), VcfReadOptions::default())
            .await
            .unwrap();

        let row_count = df.clone().count().await.unwrap();
        assert_eq!(row_count, 3, "expected 3 records from sample.vcf");

        let columns: Vec<&str> = df
            .schema()
            .fields()
            .iter()
            .map(|f| f.name().as_str())
            .collect();
        assert!(columns.contains(&"chrom"), "missing chrom column: {columns:?}");
        assert!(columns.contains(&"pos"), "missing pos column: {columns:?}");
    }

    /// Exercises the projection-pushdown path: selecting a column subset must not
    /// trip the "does not support projection pushdown" guard in FileScanConfig.
    #[tokio::test]
    async fn test_read_vcf_projection() {
        let ctx = SessionContext::new();
        let path = test_data_path("sample.vcf");
        let df = ctx
            .read_vcf(path.to_str().unwrap(), VcfReadOptions::default())
            .await
            .unwrap();

        let projected = df.select_columns(&["chrom", "pos"]).unwrap();
        let columns: Vec<&str> = projected
            .schema()
            .fields()
            .iter()
            .map(|f| f.name().as_str())
            .collect();
        assert_eq!(columns, vec!["chrom", "pos"]);

        let batches = projected.collect().await.unwrap();
        let total_rows: usize = batches.iter().map(|b| b.num_rows()).sum();
        assert_eq!(total_rows, 3);
    }

    #[tokio::test]
    async fn test_read_vcf_file_not_found() {
        let ctx = SessionContext::new();
        let result = ctx
            .read_vcf("/tmp/nonexistent_file.vcf", VcfReadOptions::default())
            .await;
        assert!(result.is_err());
    }
}
