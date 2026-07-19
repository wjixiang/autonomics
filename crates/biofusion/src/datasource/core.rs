//! Generic DataFusion file-format core for the bioinformatics formats oxbow
//! supports.
//!
//! Every format is wired into DataFusion through the same generic
//! [`FileFormat`] / [`FileSource`] / [`FileOpener`] stack implemented here:
//! each format only contributes a small [`BioDriver`] (schema inference +
//! scan). The plumbing — projection pushdown, batch sizing, listing-table
//! resolution, the writer stub — is shared.
//!
//! See the module-level guide in [`crate::datasource`] for the architecture.

use std::collections::HashMap;
use std::io::{BufRead, Cursor, Read};
use std::marker::PhantomData;
use std::sync::Arc;

use arrow::array::RecordBatch;
use arrow::error::ArrowError;
use arrow_schema::SchemaRef;
use async_trait::async_trait;
use bytes::Bytes;
use datafusion::catalog::Session;
use datafusion::common::{GetExt, Statistics};
use datafusion::datasource::file_format::file_compression_type::FileCompressionType;
use datafusion::datasource::listing::{
    ListingOptions, ListingTable, ListingTableConfig, ListingTableUrl,
};
use datafusion::datasource::physical_plan::{FileScanConfig, FileSource};
use datafusion::datasource::table_schema::TableSchema;
use datafusion::error::{DataFusionError, Result};
use datafusion::execution::context::DataFilePaths;
use datafusion::object_store::{ObjectStore, ObjectStoreExt};
use datafusion::physical_expr::LexRequirement;
use datafusion::physical_plan::ExecutionPlan;
use datafusion::physical_plan::metrics::ExecutionPlanMetricsSet;
use datafusion::prelude::{DataFrame, SessionContext};
use datafusion_datasource::file_format::{FileFormat, FileFormatFactory};
use datafusion_datasource::file_scan_config::FileScanConfigBuilder;
use datafusion_datasource::file_sink_config::FileSinkConfig;
use datafusion_datasource::projection::{ProjectionOpener, SplitProjection};
use datafusion_datasource::source::DataSourceExec;
use futures::stream::{self, StreamExt};

use datafusion::datasource::listing::PartitionedFile;
use datafusion::datasource::physical_plan::{FileOpenFuture, FileOpener};

// =====================================================================
// BioDriver — the per-format extension point
// =====================================================================

/// The bytes of a single bioinformatics object plus whether the core detected
/// gzip/BGZF framing from the magic bytes.
///
/// Drivers receive this rather than the raw [`ObjectMeta`] so that schema
/// inference and the runtime scan share one, location-independent interface.
#[derive(Debug, Clone)]
pub struct BioInput {
    /// Full object bytes.
    pub bytes: Bytes,
    /// True when the bytes start with the gzip magic (`1f 8b`), i.e. the file
    /// is gzip- or BGZF-compressed.
    pub gz: bool,
}

/// A boxed iterator over decoded record batches.
///
/// oxbow's scanners return `impl RecordBatchReader`; since
/// `RecordBatchReader: Iterator<Item = Result<RecordBatch, ArrowError>>`, any
/// such reader boxes cleanly into this type.
pub type BioBatchIter =
    Box<dyn Iterator<Item = std::result::Result<RecordBatch, ArrowError>> + Send>;

/// Per-format driver: how to infer the Arrow schema and how to scan bytes into
/// record batches.
///
/// The methods take [`BioInput`] by value/reference and do not use `&self`, so
/// a driver is a pure type-level strategy (`FILE_TYPE` + two functions). This
/// keeps the generic core free of per-format state.
pub trait BioDriver: Send + Sync + Unpin + 'static {
    /// Lowercase file type / extension base, e.g. `"vcf"`, `"fasta"`.
    const FILE_TYPE: &'static str;

    /// Infer the Arrow schema from the object's bytes (without scanning).
    fn infer_schema(input: &BioInput) -> Result<SchemaRef>;

    /// Scan the object's bytes into a [`Send`] batch iterator.
    fn scan(input: BioInput, batch_size: usize) -> Result<BioBatchIter>;
}

/// Detect gzip / BGZF framing from the leading magic bytes (`1f 8b`).
pub fn is_gzip(bytes: &[u8]) -> bool {
    bytes.len() >= 2 && bytes[0] == 0x1f && bytes[1] == 0x8b
}

/// Detect a proper BGZF stream: a gzip member that carries the BGZF extra
/// subfield (`BC`). This distinguishes block-gzip (BGZF) from a plain gzip
/// stream — both share the `1f 8b` magic, but only BGZF carries the `BC` extra
/// subfield that noodles' bgzf reader expects.
pub fn is_bgzf(bytes: &[u8]) -> bool {
    // Need at least the fixed 10-byte gzip header + 2-byte XLEN.
    if bytes.len() < 12 || bytes[0..3] != [0x1f, 0x8b, 0x08] {
        return false;
    }
    let flags = bytes[3];
    // BGZF always sets FEXTRA (0x04).
    if flags & 0x04 == 0 {
        return false;
    }
    let xlen = u16::from_le_bytes([bytes[10], bytes[11]]) as usize;
    if bytes.len() < 12 + xlen {
        return false;
    }
    let extra = &bytes[12..12 + xlen];
    // The BGZF subfield is identified by SI1=0x42, SI2=0x43 ("BC").
    extra.windows(2).any(|w| w[0] == 0x42 && w[1] == 0x43)
}

/// Wrap `bytes` as a boxed, `Send` buffered reader, transparently decompressing
/// when `gz` is true. BGZF streams go through noodles' bgzf reader (which also
/// preserves virtual positions); plain gzip streams go through flate2. Drivers
/// whose underlying noodles reader needs `R: BufRead` (VCF, FASTA, FASTQ, BED,
/// GTF, GFF, SAM) route through this.
pub fn buf_reader(bytes: Bytes, gz: bool) -> Box<dyn BufRead + Send> {
    if gz {
        if is_bgzf(&bytes) {
            Box::new(noodles::bgzf::io::Reader::new(Cursor::new(bytes)))
        } else {
            Box::new(std::io::BufReader::new(flate2::read::MultiGzDecoder::new(
                Cursor::new(bytes),
            )))
        }
    } else {
        Box::new(Cursor::new(bytes))
    }
}

/// Wrap `bytes` as a boxed, `Send` byte reader (no buffering). Drivers whose
/// noodles reader needs `R: Read` and wraps its own BGZF decoder (BCF, BAM,
/// CRAM) route through this.
pub fn byte_reader(bytes: Bytes) -> Box<dyn Read + Send> {
    Box::new(Cursor::new(bytes))
}

/// Map an [`oxbow`] / noodles error into a DataFusion external error.
pub(crate) fn map_ext<E: std::error::Error + Send + Sync + 'static>(e: E) -> DataFusionError {
    DataFusionError::External(Box::new(e))
}

// =====================================================================
// Shared options
// =====================================================================

/// Shared, per-format runtime configuration consumed by the generic core.
#[derive(Debug, Clone)]
pub struct BioOptions {
    /// Target number of rows per produced [`RecordBatch`].
    pub batch_size: usize,
    /// Maximum number of records to consider when inferring the schema.
    pub schema_infer_max_records: usize,
    /// Compression of the underlying file, if any.
    pub compression: FileCompressionType,
}

impl Default for BioOptions {
    fn default() -> Self {
        Self {
            batch_size: 8192,
            schema_infer_max_records: DEFAULT_SCHEMA_INFER_MAX_RECORD,
            compression: FileCompressionType::UNCOMPRESSED,
        }
    }
}

/// Default cap on the number of records sampled during schema inference.
const DEFAULT_SCHEMA_INFER_MAX_RECORD: usize = 1000;

/// User-facing read options shared by every `read_<format>` helper.
///
/// Mirrors DataFusion's `CsvReadOptions` shape (concrete, builder-style) but is
/// **not** a [`datafusion::execution::options::ReadOptions`]: the generic
/// [`read_bio`] builds [`ListingOptions`] directly, which avoids threading a
/// format-specific `FileFormat` through the `ReadOptions` trait.
#[derive(Clone, Debug, Default)]
pub struct BioReadOptions {
    file_extension: Option<String>,
    batch_size: Option<usize>,
    limit: Option<usize>,
    columns: Option<Vec<String>>,
}

impl BioReadOptions {
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

    pub fn with_columns(mut self, columns: Vec<String>) -> Self {
        self.columns = Some(columns);
        self
    }

    pub fn columns(&self) -> Option<&[String]> {
        self.columns.as_deref()
    }
}

// =====================================================================
// BioSource — FileSource
// =====================================================================

/// Generic [`FileSource`] for any [`BioDriver`].
///
/// Holds per-scan state (options, batch size, table schema, the current
/// projection, metrics) and is cloned cheaply whenever DataFusion applies new
/// configuration. Column projection is delegated to [`ProjectionOpener`].
pub struct BioSource<D: BioDriver> {
    options: BioOptions,
    batch_size: Option<usize>,
    table_schema: TableSchema,
    projection: SplitProjection,
    metrics: ExecutionPlanMetricsSet,
    _driver: PhantomData<D>,
}

// Manual `Clone`: `D` carries no state and need not be `Clone`.
impl<D: BioDriver> Clone for BioSource<D> {
    fn clone(&self) -> Self {
        Self {
            options: self.options.clone(),
            batch_size: self.batch_size,
            table_schema: self.table_schema.clone(),
            projection: self.projection.clone(),
            metrics: ExecutionPlanMetricsSet::new(),
            _driver: PhantomData,
        }
    }
}

impl<D: BioDriver> BioSource<D> {
    /// Create a new source for the given (unprojected) table schema.
    pub fn new(table_schema: TableSchema) -> Self {
        let projection = SplitProjection::unprojected(&table_schema);
        Self {
            options: BioOptions::default(),
            batch_size: None,
            table_schema,
            projection,
            metrics: ExecutionPlanMetricsSet::new(),
            _driver: PhantomData,
        }
    }

    /// Attach format options to this source.
    pub fn with_options(mut self, options: BioOptions) -> Self {
        self.options = options;
        self
    }

    /// Borrow the configured options.
    pub fn options(&self) -> &BioOptions {
        &self.options
    }
}

impl<D: BioDriver + 'static> FileSource for BioSource<D> {
    /// Bioinformatics formats (VCF, BAM, FASTA, …) are **not** byte-range
    /// splittable — record boundaries depend on format-specific framing
    /// (VCF lines, BGZF virtual offsets, FASTA `>`, BAM alignment blocks).
    /// The opener reads the entire object and feeds it to the format-specific
    /// decoder, so DataFusion's byte-range repartitioner would create N
    /// partitions that each re-read and re-scan the same file, multiplying
    /// every row N ×.
    fn supports_repartitioning(&self) -> bool {
        false
    }

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
        let file_indices = self.projection.file_indices.clone();
        let opener: Arc<dyn FileOpener> =
            Arc::new(BioOpener::<D>::new(object_store, options, file_indices));
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
        projection: &datafusion::physical_expr::projection::ProjectionExprs,
    ) -> Result<Option<Arc<dyn FileSource>>> {
        let mut source = self.clone();
        let merged = self.projection.source.try_merge(projection)?;
        source.projection = SplitProjection::new(self.table_schema.file_schema(), &merged);
        Ok(Some(Arc::new(source)))
    }

    fn projection(&self) -> Option<&datafusion::physical_expr::projection::ProjectionExprs> {
        Some(&self.projection.source)
    }

    fn metrics(&self) -> &ExecutionPlanMetricsSet {
        &self.metrics
    }

    fn file_type(&self) -> &str {
        D::FILE_TYPE
    }
}

// =====================================================================
// BioOpener — FileOpener
// =====================================================================

/// [`FileOpener`] for a single file of any [`BioDriver`] format.
///
/// Fetches the whole object (these formats cannot generally be split on
/// arbitrary byte boundaries), decodes it through the driver, and yields a
/// [`RecordBatch`] stream. Column projection is applied by the wrapping
/// [`ProjectionOpener`].
pub struct BioOpener<D: BioDriver> {
    object_store: Arc<dyn ObjectStore>,
    options: BioOptions,
    file_indices: Vec<usize>,
    _driver: PhantomData<D>,
}

impl<D: BioDriver> BioOpener<D> {
    pub fn new(
        object_store: Arc<dyn ObjectStore>,
        options: BioOptions,
        file_indices: Vec<usize>,
    ) -> Self {
        Self {
            object_store,
            options,
            _driver: PhantomData,
            file_indices,
        }
    }
}

impl<D: BioDriver + 'static> FileOpener for BioOpener<D> {
    fn open(&self, partitioned_file: PartitionedFile) -> Result<FileOpenFuture> {
        let store = Arc::clone(&self.object_store);
        let batch_size = self.options.batch_size;
        let indices = self.file_indices.clone();

        Ok(Box::pin(async move {
            let location = partitioned_file.object_meta.location.clone();
            let bytes = store.get(&location).await?.bytes().await?;
            let input = BioInput {
                gz: is_gzip(&bytes),
                bytes,
            };
            // scan returns a synchronous iterator over Result<RecordBatch, _>.
            let batches = D::scan(input, batch_size)?;
            let stream = stream::iter(batches)
                .map(move |r| {
                    let batch = r?;
                    if indices.len() < batch.num_columns() {
                        batch.project(&indices).map_err(DataFusionError::from)
                    } else {
                        Ok(batch)
                    }
                })
                .boxed();
            Ok(stream)
        }))
    }
}

// =====================================================================
// BioFormat — FileFormat
// =====================================================================

/// [`FileFormat`] for any [`BioDriver`].
pub struct BioFormat<D: BioDriver> {
    options: BioOptions,
    _driver: PhantomData<D>,
}

impl<D: BioDriver> std::fmt::Debug for BioFormat<D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BioFormat")
            .field("format", &D::FILE_TYPE)
            .field("options", &self.options)
            .finish()
    }
}

impl<D: BioDriver> Default for BioFormat<D> {
    fn default() -> Self {
        Self::new()
    }
}

impl<D: BioDriver> BioFormat<D> {
    pub fn new() -> Self {
        Self {
            options: BioOptions::default(),
            _driver: PhantomData,
        }
    }

    pub fn with_options(mut self, options: BioOptions) -> Self {
        self.options = options;
        self
    }

    pub fn options(&self) -> &BioOptions {
        &self.options
    }
}

#[async_trait]
impl<D: BioDriver + 'static> FileFormat for BioFormat<D> {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn get_ext(&self) -> String {
        D::FILE_TYPE.to_string()
    }

    fn get_ext_with_compression(
        &self,
        _file_compression_type: &FileCompressionType,
    ) -> Result<String> {
        Ok(D::FILE_TYPE.to_string())
    }

    fn compression_type(&self) -> Option<FileCompressionType> {
        if self.options.compression.is_compressed() {
            Some(self.options.compression)
        } else {
            None
        }
    }

    async fn infer_schema(
        &self,
        _state: &dyn Session,
        store: &Arc<dyn ObjectStore>,
        objects: &[datafusion::object_store::ObjectMeta],
    ) -> Result<SchemaRef> {
        let object = objects.first().ok_or_else(|| {
            DataFusionError::Plan(format!(
                "{} schema inference received no objects",
                D::FILE_TYPE
            ))
        })?;
        let bytes = store.get(&object.location).await?.bytes().await?;
        let input = BioInput {
            gz: is_gzip(&bytes),
            bytes,
        };
        D::infer_schema(&input)
    }

    async fn infer_stats(
        &self,
        _state: &dyn Session,
        _store: &Arc<dyn ObjectStore>,
        table_schema: SchemaRef,
        _object: &datafusion::object_store::ObjectMeta,
    ) -> Result<Statistics> {
        Ok(Statistics::new_unknown(&table_schema))
    }

    async fn create_physical_plan(
        &self,
        _state: &dyn Session,
        conf: FileScanConfig,
    ) -> Result<Arc<dyn ExecutionPlan>> {
        // The ListingTable places a BioSource<D> on the incoming config; rebind
        // it with this format's options and rebuild the scan as a DataSourceExec.
        let source = conf
            .file_source
            .as_any()
            .downcast_ref::<BioSource<D>>()
            .ok_or_else(|| {
                DataFusionError::Internal(format!(
                    "{} format received a file_source that is not a BioSource<{}>",
                    D::FILE_TYPE,
                    D::FILE_TYPE
                ))
            })?;
        let source = Arc::new(source.clone().with_options(self.options.clone()));
        let config = FileScanConfigBuilder::from(conf)
            .with_source(source)
            .build();
        Ok(DataSourceExec::from_data_source(config))
    }

    fn file_source(&self, table_schema: TableSchema) -> Arc<dyn FileSource> {
        Arc::new(BioSource::<D>::new(table_schema).with_options(self.options.clone()))
    }

    /// Writing these formats from an arbitrary [`RecordBatch`] is not supported.
    async fn create_writer_physical_plan(
        &self,
        _input: Arc<dyn ExecutionPlan>,
        _state: &dyn Session,
        _conf: FileSinkConfig,
        _order_requirements: Option<LexRequirement>,
    ) -> Result<Arc<dyn ExecutionPlan>> {
        Err(DataFusionError::NotImplemented(format!(
            "{} write is not supported",
            D::FILE_TYPE
        )))
    }
}

// =====================================================================
// BioFormatFactory — FileFormatFactory + GetExt
// =====================================================================

/// Factory used to register a format with a `SessionContext` and to materialize
/// a [`BioFormat`] from SQL `OPTIONS`.
pub struct BioFormatFactory<D: BioDriver> {
    _driver: PhantomData<D>,
}

impl<D: BioDriver> Default for BioFormatFactory<D> {
    fn default() -> Self {
        Self {
            _driver: PhantomData,
        }
    }
}

impl<D: BioDriver> std::fmt::Debug for BioFormatFactory<D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BioFormatFactory")
            .field("format", &D::FILE_TYPE)
            .finish()
    }
}

impl<D: BioDriver + 'static> FileFormatFactory for BioFormatFactory<D> {
    fn create(
        &self,
        _state: &dyn Session,
        format_options: &HashMap<String, String>,
    ) -> Result<Arc<dyn FileFormat>> {
        let mut options = BioOptions::default();
        if let Some(batch_size) = format_options
            .get("batch_size")
            .and_then(|v| v.parse().ok())
        {
            options.batch_size = batch_size;
        }
        if let Some(max_records) = format_options
            .get("schema_infer_max_records")
            .and_then(|v| v.parse().ok())
        {
            options.schema_infer_max_records = max_records;
        }
        Ok(Arc::new(BioFormat::<D>::new().with_options(options)))
    }

    fn default(&self) -> Arc<dyn FileFormat> {
        Arc::new(BioFormat::<D>::new())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl<D: BioDriver> GetExt for BioFormatFactory<D> {
    fn get_ext(&self) -> String {
        D::FILE_TYPE.to_string()
    }
}

// =====================================================================
// read_bio — ListingTable entry point
// =====================================================================

/// Derive the ListingTable file extension for a format from the input URL.
///
/// ListingTable filters discovered files by this extension, so it must reflect
/// the *actual* file on disk — including non-canonical suffixes like `.bw`
/// (BigWig) or `.bb` (BigBed) that differ from the format's `FILE_TYPE`, and a
/// `.gz` suffix for compressed inputs. If the user set an explicit extension we
/// keep it; otherwise we infer it from the path, falling back to `base_ext`
/// (the format's `FILE_TYPE`) when the path has no extension.
pub(crate) fn infer_file_extension(
    url: &ListingTableUrl,
    explicit: Option<&str>,
    base_ext: &str,
) -> String {
    if let Some(ext) = explicit {
        return ext.to_string();
    }
    let s = url.as_str();
    let name = s.rsplit('/').next().unwrap_or(s);
    // Strip any trailing query/fragment before inspecting the suffix.
    let name = name.split(['?', '#']).next().unwrap_or(name);
    if let Some(stripped) = name.strip_suffix(".gz") {
        // e.g. "sample.vcf.gz" -> "vcf.gz"; "sample.gz" -> "gz"
        return match stripped.rfind('.') {
            Some(idx) => format!("{}.gz", &stripped[idx + 1..]),
            None => "gz".to_string(),
        };
    }
    if let Some(idx) = name.rfind('.') {
        return name[idx + 1..].to_string();
    }
    base_ext.to_string()
}

/// Build a [`DataFrame`] over `paths` for the format driven by `D`, mirroring
/// DataFusion's private `_read_type` helper: resolve URLs, infer the schema,
/// and wrap a [`ListingTable`] via [`SessionContext::read_table`].
pub async fn read_bio<D, P>(
    ctx: &SessionContext,
    table_paths: P,
    options: BioReadOptions,
) -> Result<DataFrame>
where
    D: BioDriver + 'static,
    P: DataFilePaths + Send,
{
    let urls = table_paths.to_urls()?;
    let url = urls.first().ok_or_else(|| {
        DataFusionError::Plan(format!("read_{}: no table path provided", D::FILE_TYPE))
    })?;

    // Auto-detect the file extension (incl. `.gz`) so ListingTable matches
    // compressed inputs when the caller didn't set one explicitly.
    let ext = infer_file_extension(url, options.file_extension(), D::FILE_TYPE);
    let mut bio_options = BioOptions::default();
    if let Some(batch_size) = options.batch_size {
        bio_options.batch_size = batch_size;
    }

    let format = Arc::new(BioFormat::<D>::new().with_options(bio_options));
    let listing_options = ListingOptions::new(format)
        .with_file_extension(ext)
        .with_collect_stat(false);

    let state = ctx.state();
    let resolved_schema = listing_options.infer_schema(&state, url).await?;

    let config = ListingTableConfig::new_with_multi_paths(urls)
        .with_listing_options(listing_options)
        .with_schema(resolved_schema);
    let provider = ListingTable::try_new(config)?;
    ctx.read_table(Arc::new(provider))
}
