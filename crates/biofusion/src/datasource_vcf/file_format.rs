//! VCF file format for DataFusion.
//!
//! This module wires the VCF format into DataFusion's file-format machinery.
//! It contains the following components (see the module-level guide in
//! `datasource_vcf.rs` for the full architecture):
//!
//! | Component        | Role                                                   |
//! |------------------|--------------------------------------------------------|
//! | [`VcfOptions`]   | Shared, serializable VCF configuration.               |
//! | [`VcfFormat`]    | [`FileFormat`] — schema inference, planning, source.   |
//! | [`VcfFormatFactory`] | [`FileFormatFactory`] + [`GetExt`] — registration.|
//! | [`VcfDecoder`]   | [`Decoder`] — bytes → [`RecordBatch`].                 |
//! | [`VcfSerializer`]| [`BatchSerializer`] — [`RecordBatch`] → bytes (write). |
//! | [`VcfSink`]      | [`FileSink`] — write target (`COPY ... TO`).           |

use std::collections::HashMap;
use std::sync::Arc;

use arrow::array::RecordBatch;
use arrow::error::ArrowError;
use arrow_schema::SchemaRef;
use async_trait::async_trait;
use bytes::Bytes;
use datafusion::common::{GetExt, Statistics};
use datafusion::datasource::file_format::file_compression_type::FileCompressionType;
use datafusion::datasource::physical_plan::{FileScanConfig, FileSource};
use datafusion::datasource::table_schema::TableSchema;
use datafusion::error::{DataFusionError, Result};
use datafusion::execution::{SendableRecordBatchStream, TaskContext};
use datafusion::object_store::{ObjectMeta, ObjectStore, ObjectStoreExt};
use datafusion::physical_expr::LexRequirement;
use datafusion::physical_plan::metrics::MetricsSet;
use datafusion::physical_plan::{DisplayAs, DisplayFormatType, ExecutionPlan};
use datafusion::catalog::Session;
use datafusion_common_runtime::SpawnedTask;
use datafusion_datasource::decoder::Decoder;
use datafusion_datasource::file_format::{FileFormat, FileFormatFactory};
use datafusion_datasource::file_scan_config::FileScanConfigBuilder;
use datafusion_datasource::file_sink_config::{FileSink, FileSinkConfig};
use datafusion_datasource::sink::{DataSink, DataSinkExec};
use datafusion_datasource::source::DataSourceExec;
use datafusion_datasource::write::{demux::DemuxedStreamReceiver, BatchSerializer};

use crate::datasource_vcf::source::{scanner_from_header, vcf_reader, VcfOpener, VcfSource};

/// File extension (without the leading dot) used to register and discover VCF.
const VCF_EXTENSION: &str = "vcf";

/// Shared configuration for the VCF format.
///
/// This is the single source of truth consumed by [`VcfFormat`], [`VcfSource`],
/// [`VcfOpener`] and [`VcfDecoder`].
#[derive(Debug, Clone)]
pub struct VcfOptions {
    /// Maximum number of records to read when inferring the schema.
    pub schema_infer_max_records: usize,
    /// Target number of rows per produced [`RecordBatch`].
    pub batch_size: usize,
    /// Compression of the underlying file, if any.
    pub compression: FileCompressionType,
}

impl Default for VcfOptions {
    fn default() -> Self {
        Self {
            schema_infer_max_records: DEFAULT_SCHEMA_INFER_MAX_RECORD,
            batch_size: 8192,
            compression: FileCompressionType::UNCOMPRESSED,
        }
    }
}

/// Default cap on the number of records sampled during schema inference.
const DEFAULT_SCHEMA_INFER_MAX_RECORD: usize = 1000;

// =====================================================================
// VcfFormat — FileFormat
// =====================================================================

/// [`FileFormat`] implementation for VCF.
///
/// Owns the format [`VcfOptions`] and is responsible for schema/stat inference
/// and for constructing the scan and writer physical plans.
#[derive(Debug, Default)]
pub struct VcfFormat {
    options: VcfOptions,
}

impl VcfFormat {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_options(mut self, options: VcfOptions) -> Self {
        self.options = options;
        self
    }

    pub fn options(&self) -> &VcfOptions {
        &self.options
    }
}

#[async_trait]
impl FileFormat for VcfFormat {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn get_ext(&self) -> String {
        VCF_EXTENSION.to_string()
    }

    fn get_ext_with_compression(
        &self,
        file_compression_type: &FileCompressionType,
    ) -> Result<String> {
        // VCF only supports (B)GZIP compression; other types are reported as-is.
        if file_compression_type.is_compressed() {
            Ok(format!("{VCF_EXTENSION}.gz"))
        } else {
            Ok(VCF_EXTENSION.to_string())
        }
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
        objects: &[ObjectMeta],
    ) -> Result<SchemaRef> {
        // The Arrow schema is fully determined by the VCF header, so a single
        // sample object is enough — no record needs to be scanned.
        let object = objects.first().ok_or_else(|| {
            DataFusionError::Plan("VCF schema inference received no objects".to_string())
        })?;
        let bytes = store.get(&object.location).await?.bytes().await?;
        let is_gz = object.location.extension() == Some("gz");

        let mut reader = vcf_reader(bytes, is_gz);
        let header = reader.read_header()?;
        let scanner = scanner_from_header(header)?;
        Ok(Arc::new(scanner.schema().clone()))
    }

    async fn infer_stats(
        &self,
        _state: &dyn Session,
        _store: &Arc<dyn ObjectStore>,
        table_schema: SchemaRef,
        _object: &ObjectMeta,
    ) -> Result<Statistics> {
        // VCF carries no cheaply-computable file-level statistics.
        Ok(Statistics::new_unknown(&table_schema))
    }

    async fn create_physical_plan(
        &self,
        _state: &dyn Session,
        conf: FileScanConfig,
    ) -> Result<Arc<dyn ExecutionPlan>> {
        // The ListingTable places a `VcfSource` on the incoming config; rebind it
        // with this format's options and rebuild the scan as a DataSourceExec.
        let source = conf
            .file_source
            .as_any()
            .downcast_ref::<VcfSource>()
            .ok_or_else(|| {
                DataFusionError::Internal(
                    "VcfFormat received a file_source that is not a VcfSource".to_string(),
                )
            })?;
        let source = Arc::new(source.clone().with_options(self.options.clone()));
        let config = FileScanConfigBuilder::from(conf)
            .with_source(source)
            .build();
        Ok(DataSourceExec::from_data_source(config))
    }

    fn file_source(&self, table_schema: TableSchema) -> Arc<dyn FileSource> {
        Arc::new(VcfSource::new(table_schema).with_options(self.options.clone()))
    }

    /// Build the writer physical plan used by `COPY ... TO` / `INSERT`.
    ///
    /// Writing VCF from an arbitrary [`RecordBatch`] is not supported yet.
    async fn create_writer_physical_plan(
        &self,
        _input: Arc<dyn ExecutionPlan>,
        _state: &dyn Session,
        _conf: FileSinkConfig,
        _order_requirements: Option<LexRequirement>,
    ) -> Result<Arc<dyn ExecutionPlan>> {
        Err(DataFusionError::NotImplemented(
            "VCF write is not supported".to_string(),
        ))
    }
}

// =====================================================================
// VcfFormatFactory — FileFormatFactory + GetExt
// =====================================================================

/// Factory used to register the VCF format with a `SessionContext` and to
/// materialize a [`VcfFormat`] from SQL `OPTIONS`.
#[derive(Debug, Default)]
pub struct VcfFormatFactory;

impl VcfFormatFactory {
    pub fn new() -> Self {
        Self
    }
}

impl FileFormatFactory for VcfFormatFactory {
    fn create(
        &self,
        _state: &dyn Session,
        format_options: &HashMap<String, String>,
    ) -> Result<Arc<dyn FileFormat>> {
        let mut options = VcfOptions::default();
        if let Some(batch_size) = format_options.get("batch_size").and_then(|v| v.parse().ok()) {
            options.batch_size = batch_size;
        }
        if let Some(max_records) = format_options
            .get("schema_infer_max_records")
            .and_then(|v| v.parse().ok())
        {
            options.schema_infer_max_records = max_records;
        }
        Ok(Arc::new(VcfFormat::new().with_options(options)))
    }

    fn default(&self) -> Arc<dyn FileFormat> {
        Arc::new(VcfFormat::new())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl GetExt for VcfFormatFactory {
    fn get_ext(&self) -> String {
        VCF_EXTENSION.to_string()
    }
}

// =====================================================================
// VcfDecoder — Decoder (bytes -> RecordBatch)
// =====================================================================

/// Streaming decoder that turns raw VCF bytes into [`RecordBatch`]es.
///
/// Intended to be driven by DataFusion's `DecoderDeserializer` /
/// `deserialize_stream` helpers inside [`VcfOpener`].
//
// Fields are read by the (still stubbed) trait method bodies; silence dead-code
// analysis until the implementations land.
#[allow(dead_code)]
#[derive(Debug)]
pub struct VcfDecoder {
    schema: SchemaRef,
    options: VcfOptions,
}

impl VcfDecoder {
    pub fn new(schema: SchemaRef, options: VcfOptions) -> Self {
        Self { schema, options }
    }
}

impl Decoder for VcfDecoder {
    /// Consume `buf`, returning the number of bytes consumed.
    fn decode(&mut self, _buf: &[u8]) -> std::result::Result<usize, ArrowError> {
        todo!("Feed bytes into the underlying VCF parser, return bytes consumed")
    }

    /// Flush any buffered records into a [`RecordBatch`].
    fn flush(&mut self) -> std::result::Result<Option<RecordBatch>, ArrowError> {
        todo!("Drain buffered records into a RecordBatch (or None when empty)")
    }

    /// Whether a batch may be emitted before the input is fully consumed.
    fn can_flush_early(&self) -> bool {
        todo!("Return true once enough records are buffered for a full batch")
    }
}

// =====================================================================
// VcfSerializer — BatchSerializer (RecordBatch -> bytes, write path)
// =====================================================================

/// Serializer that encodes [`RecordBatch`]es back into the VCF byte format.
//
// Field is read by the (still stubbed) trait method body; silence dead-code
// analysis until the implementation lands.
#[allow(dead_code)]
#[derive(Debug)]
pub struct VcfSerializer {
    schema: SchemaRef,
}

impl VcfSerializer {
    pub fn new(schema: SchemaRef) -> Self {
        Self { schema }
    }
}

impl Default for VcfSerializer {
    fn default() -> Self {
        Self {
            schema: Arc::new(arrow_schema::Schema::empty()),
        }
    }
}

impl BatchSerializer for VcfSerializer {
    /// Serialize `batch` into VCF bytes. `initial` is true for the first batch
    /// of a file (where the VCF header must be emitted).
    fn serialize(&self, _batch: RecordBatch, _initial: bool) -> Result<Bytes> {
        todo!("Serialize the RecordBatch to VCF bytes (write header when initial)")
    }
}

// =====================================================================
// VcfSink — FileSink (DataSink) write target
// =====================================================================

/// Sink that writes incoming [`RecordBatch`]es to VCF files via `COPY ... TO`.
pub struct VcfSink {
    config: FileSinkConfig,
}

impl VcfSink {
    pub fn new(config: FileSinkConfig) -> Self {
        Self { config }
    }
}

impl std::fmt::Debug for VcfSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VcfSink").finish()
    }
}

impl DisplayAs for VcfSink {
    fn fmt_as(
        &self,
        _t: DisplayFormatType,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        write!(f, "VcfSink")
    }
}

#[async_trait]
impl DataSink for VcfSink {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn metrics(&self) -> Option<MetricsSet> {
        None
    }

    fn schema(&self) -> &SchemaRef {
        todo!("Return the output schema from self.config")
    }

    /// Delegates to the default [`FileSink::write_all`] implementation.
    async fn write_all(
        &self,
        _data: SendableRecordBatchStream,
        _context: &Arc<TaskContext>,
    ) -> Result<u64> {
        todo!("Delegate to FileSink::write_all via spawn_writer_tasks_and_join")
    }
}

#[async_trait]
impl FileSink for VcfSink {
    fn config(&self) -> &FileSinkConfig {
        &self.config
    }

    async fn spawn_writer_tasks_and_join(
        &self,
        _context: &Arc<TaskContext>,
        _demux_task: SpawnedTask<std::result::Result<(), DataFusionError>>,
        _file_stream_rx: DemuxedStreamReceiver,
        _object_store: Arc<dyn ObjectStore>,
    ) -> Result<u64> {
        todo!(
            "Build a VcfSerializer + compression from config and delegate to \
             datafusion::datasource::write::orchestration::spawn_writer_tasks_and_join"
        )
    }
}
