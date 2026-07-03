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

use crate::datasource_bcf::file_format::{BcfFormat, BcfOptions};

/// Options for reading BCF files.
///
/// Follows the same pattern as DataFusion's `CsvReadOptions`, `ParquetReadOptions`, etc.:
/// concrete type, not `impl ReadOptions<'a>`.
#[derive(Clone, Debug, Default)]
pub struct BcfReadOptions<'a> {
    file_extension: Option<String>,
    batch_size: Option<usize>,
    limit: Option<usize>,
    columns: Option<Vec<&'a str>>,
}

impl<'a> BcfReadOptions<'a> {
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
impl<'a> ReadOptions<'a> for BcfReadOptions<'a> {
    #[doc = " Helper to convert these user facing options to `ListingTable` options"]
    fn to_listing_options(
        &self,
        _config: &SessionConfig,
        _table_options: TableOptions,
    ) -> ListingOptions {
        let mut bcf_options = BcfOptions::default();
        if let Some(batch_size) = self.batch_size {
            bcf_options.batch_size = batch_size;
        }
        let format = Arc::new(BcfFormat::new().with_options(bcf_options));
        ListingOptions::new(format)
            .with_file_extension(self.file_extension.clone().unwrap_or_else(|| "bcf".to_string()))
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
    use std::io::{Cursor, Write};

    use datafusion::prelude::SessionContext;
    use noodles::bcf;
    use noodles::vcf;
    use noodles::vcf::variant::io::Write as _;

    use crate::datasource_bcf::BcfReadOptions;
    use crate::ext::DataFusionReadExt;

    fn test_data_path(file: &str) -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_datasets")
            .join(file)
    }

    /// Convert the committed `sample.vcf` fixture into a real BCF file at test
    /// time (BCF is binary, so we cannot check in a hand-written fixture).
    /// Returns the path to the generated `.bcf` under the temp dir.
    fn write_sample_bcf() -> std::path::PathBuf {
        let vcf_path = test_data_path("sample.vcf");
        let vcf_bytes = std::fs::read(&vcf_path).unwrap();

        let mut vcf_reader = vcf::io::Reader::new(Cursor::new(vcf_bytes));
        let header = vcf_reader.read_header().unwrap();

        let mut bcf_writer = bcf::io::Writer::new(Vec::new());
        bcf_writer.write_variant_header(&header).unwrap();
        for record in vcf_reader.records() {
            let record = record.unwrap();
            bcf_writer.write_variant_record(&header, &record).unwrap();
        }
        bcf_writer.try_finish().unwrap();
        let bytes = bcf_writer.into_inner().into_inner();

        let path = std::env::temp_dir().join("biofusion_sample.bcf");
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(&bytes).unwrap();
        path
    }

    #[tokio::test]
    async fn test_read_bcf_plain() {
        let path = write_sample_bcf();
        let ctx = SessionContext::new();
        let df = ctx
            .read_bcf(path.to_str().unwrap(), BcfReadOptions::default())
            .await
            .unwrap();

        let row_count = df.clone().count().await.unwrap();
        assert_eq!(row_count, 3, "expected 3 records from sample.bcf");

        let columns: Vec<&str> = df
            .schema()
            .fields()
            .iter()
            .map(|f| f.name().as_str())
            .collect();
        assert!(columns.contains(&"chrom"), "missing chrom column: {columns:?}");
        assert!(columns.contains(&"pos"), "missing pos column: {columns:?}");
    }

    #[tokio::test]
    async fn test_read_bcf_projection() {
        let path = write_sample_bcf();
        let ctx = SessionContext::new();
        let df = ctx
            .read_bcf(path.to_str().unwrap(), BcfReadOptions::default())
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
    async fn test_read_bcf_file_not_found() {
        let ctx = SessionContext::new();
        let result = ctx
            .read_bcf("/tmp/nonexistent_file.bcf", BcfReadOptions::default())
            .await;
        assert!(result.is_err());
    }
}
