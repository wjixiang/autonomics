//! File source and opener for VCF files.
//!
//! Implements the runtime side of the VCF file format:
//! - [`VcfSource`] implements [`FileSource`] and is responsible for creating
//!   an opener and exposing schema / metrics / configuration. Column projection
//!   is delegated to [`ProjectionOpener`].
//! - [`VcfOpener`] implements [`FileOpener`] and is responsible for opening a
//!   single partitioned file and producing a stream of [`RecordBatch`] via the
//!   oxbow VCF scanner.

use std::io::{BufRead, Cursor};
use std::sync::Arc;

use arrow::array::RecordBatch;
use bytes::Bytes;
use datafusion::datasource::listing::PartitionedFile;
use datafusion::datasource::physical_plan::{
    FileOpenFuture, FileOpener, FileScanConfig, FileSource,
};
use datafusion::datasource::table_schema::TableSchema;
use datafusion::error::{DataFusionError, Result};
use datafusion::object_store::{ObjectStore, ObjectStoreExt};
use datafusion::physical_expr::projection::ProjectionExprs;
use datafusion::physical_plan::metrics::ExecutionPlanMetricsSet;
use datafusion_datasource::projection::{ProjectionOpener, SplitProjection};
use futures::stream::{self, StreamExt};
use oxbow::variant::VcfScanner;
use oxbow::{CoordSystem, Select};

use crate::datasource_vcf::file_format::VcfOptions;

/// Build a noodles VCF reader over `bytes`, transparently BGZF-decoding when
/// `gz` is true. The inner reader is boxed so plain and compressed inputs share
/// one concrete [`noodles::vcf::io::Reader`] type.
pub(crate) fn vcf_reader(
    bytes: Bytes,
    gz: bool,
) -> noodles::vcf::io::Reader<Box<dyn BufRead + Send>> {
    let cursor = Cursor::new(bytes);
    let inner: Box<dyn BufRead + Send> = if gz {
        Box::new(noodles::bgzf::io::Reader::new(cursor))
    } else {
        Box::new(cursor)
    };
    noodles::vcf::io::Reader::new(inner)
}

/// Construct an oxbow [`VcfScanner`] from a parsed VCF header, selecting every
/// contig / field / sample (full schema). Used by both schema inference and the
/// runtime opener so they agree on the produced schema.
pub(crate) fn scanner_from_header(header: noodles::vcf::Header) -> Result<VcfScanner> {
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
    .map_err(|e| DataFusionError::External(Box::new(e)))
}

/// [`FileSource`] implementation for the VCF format.
///
/// Holds the per-scan configuration (options, batch size, table schema, the
/// current projection and a metrics set). It is cloned cheaply whenever
/// DataFusion needs to apply new configuration (projection pushdown, batch size
/// override).
#[derive(Clone)]
pub struct VcfSource {
    options: VcfOptions,
    batch_size: Option<usize>,
    table_schema: TableSchema,
    projection: SplitProjection,
    metrics: ExecutionPlanMetricsSet,
}

impl VcfSource {
    /// Create a new source for the given (unprojected) table schema.
    pub fn new(table_schema: TableSchema) -> Self {
        let projection = SplitProjection::unprojected(&table_schema);
        Self {
            options: VcfOptions::default(),
            batch_size: None,
            table_schema,
            projection,
            metrics: ExecutionPlanMetricsSet::new(),
        }
    }

    /// Attach VCF-specific options to this source.
    pub fn with_options(mut self, options: VcfOptions) -> Self {
        self.options = options;
        self
    }

    /// Borrow the configured options.
    pub fn options(&self) -> &VcfOptions {
        &self.options
    }
}

impl FileSource for VcfSource {
    fn create_file_opener(
        &self,
        object_store: Arc<dyn ObjectStore>,
        _base_config: &FileScanConfig,
        _partition: usize,
    ) -> Result<Arc<dyn FileOpener>> {
        let mut options = self.options.clone();
        if let Some(batch_size) = self.batch_size {
            options.batch_size = batch_size;
        }
        let opener: Arc<dyn FileOpener> = Arc::new(VcfOpener::new(object_store, options));
        // The opener yields the full file schema; ProjectionOpener applies the
        // column projection (and resolves partition-column references).
        ProjectionOpener::try_new(
            self.projection.clone(),
            opener,
            self.table_schema.file_schema(),
        )
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn table_schema(&self) -> &TableSchema {
        &self.table_schema
    }

    fn with_batch_size(&self, batch_size: usize) -> Arc<dyn FileSource> {
        let mut source = self.clone();
        source.batch_size = Some(batch_size);
        Arc::new(source)
    }

    fn try_pushdown_projection(
        &self,
        projection: &ProjectionExprs,
    ) -> Result<Option<Arc<dyn FileSource>>> {
        let mut source = self.clone();
        let merged = self.projection.source.try_merge(projection)?;
        source.projection = SplitProjection::new(self.table_schema.file_schema(), &merged);
        Ok(Some(Arc::new(source)))
    }

    fn projection(&self) -> Option<&ProjectionExprs> {
        Some(&self.projection.source)
    }

    fn metrics(&self) -> &ExecutionPlanMetricsSet {
        &self.metrics
    }

    fn file_type(&self) -> &str {
        "vcf"
    }
}

/// [`FileOpener`] implementation for a single VCF file.
///
/// Fetches the whole object (VCF records cannot generally be split on arbitrary
/// byte boundaries), parses it through noodles/oxbow and yields a
/// [`RecordBatch`] stream. Column projection is applied by the wrapping
/// [`ProjectionOpener`].
pub struct VcfOpener {
    object_store: Arc<dyn ObjectStore>,
    options: VcfOptions,
}

impl VcfOpener {
    pub fn new(object_store: Arc<dyn ObjectStore>, options: VcfOptions) -> Self {
        Self {
            object_store,
            options,
        }
    }
}

impl FileOpener for VcfOpener {
    /// Open `partitioned_file` and return a future that yields the batch stream.
    fn open(&self, partitioned_file: PartitionedFile) -> Result<FileOpenFuture> {
        let store = Arc::clone(&self.object_store);
        let batch_size = self.options.batch_size;

        Ok(Box::pin(async move {
            let location = partitioned_file.object_meta.location.clone();
            let is_gz = location.extension() == Some("gz");

            let bytes = store.get(&location).await?.bytes().await?;

            let mut reader = vcf_reader(bytes, is_gz);
            let header = reader.read_header()?;
            let scanner = scanner_from_header(header)?;
            // scan consumes the reader and returns a synchronous iterator over
            // Result<RecordBatch, ArrowError>.
            let batches = scanner
                .scan(reader, None, Some(batch_size), None)
                .map_err(|e| DataFusionError::External(Box::new(e)))?;

            let stream = stream::iter(batches)
                .map(|r| r.map_err(DataFusionError::from))
                .boxed();
            Ok(stream)
        }))
    }
}
