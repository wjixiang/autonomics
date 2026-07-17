//! Unified sink node: consumes an upstream `DataFrame` and writes it out.
//!
//! Symmetric to [`crate::data_engine::nodes::SourceNode`]: a [`SinkNode`] has
//! exactly one input and produces no output. The destination is described by
//! [`Sink`] — a file (CSV / Parquet) or an Iceberg table.

use std::sync::Arc;

use async_trait::async_trait;
use datafusion::common::HashMap;
use datafusion::common::config::{CsvOptions, TableParquetOptions};
use datafusion::dataframe::{DataFrame, DataFrameWriteOptions};
use datafusion::prelude::SessionContext;
use datalake::Datalake;
use iceberg::Catalog;
use iceberg::arrow::FieldMatchMode;
use iceberg::arrow::arrow_schema_to_schema_auto_assign_ids;
use iceberg::spec::DataFileFormat;
use iceberg::transaction::{ApplyTransactionAction, Transaction};
use iceberg::writer::base_writer::data_file_writer::DataFileWriterBuilder;
use iceberg::writer::file_writer::ParquetWriterBuilder;
use iceberg::writer::file_writer::location_generator::{
    DefaultFileNameGenerator, DefaultLocationGenerator,
};
use iceberg::writer::file_writer::rolling_writer::RollingFileWriterBuilder;
use iceberg::writer::partitioning::unpartitioned_writer::UnpartitionedWriter;
use iceberg::{NamespaceIdent, TableCreation};
use parquet::file::properties::WriterProperties;
use thiserror::Error;

use super::meta::{DagNode, NodeInput, NodeMeta};
use super::source::normalize_path;
use crate::data_engine::dag::DagError;
use crate::data_engine::dag::graph::PortOutputs;

/// Where a [`SinkNode`] writes to.
#[derive(Debug, Clone)]
pub enum Sink {
    /// Write to a file path or URL.
    File { path: String, format: WriteFormat },
    /// Write to an Iceberg table (catalog write path must be available).
    Iceberg { ident: String },
}

/// Supported on-disk write formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteFormat {
    Csv,
    Parquet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SinkMode {
    /// Add the new rows after whatever is already at the destination.
    Append,
    /// Replace whatever is at the destination with the new rows.
    #[default]
    Overwrite,
}

#[derive(Debug, Error)]
pub enum SinkError {
    #[error("Invalid input: {message}")]
    InvalidInput { message: String },
    #[error("write sink '{path}' failed")]
    Write {
        path: String,
        #[source]
        source: datafusion::error::DataFusionError,
    },
    #[error("Sink to Iceberg error: {msg}")]
    Iceberg { msg: String },
}

impl From<SinkError> for DagError {
    fn from(e: SinkError) -> Self {
        match e {
            SinkError::Write { source, .. } => DagError::DataFusion(source),
            SinkError::InvalidInput { message } => DagError::Schedule(message),
            SinkError::Iceberg { msg } => DagError::NodeError {
                node_type: "sink".to_string(),
                msg,
            },
        }
    }
}

pub struct SinkNode {
    meta: NodeMeta,
    sink: Sink,
    mode: SinkMode,
    ctx: SessionContext,
    datalake: Arc<Datalake>,
}

impl SinkNode {
    pub fn new(
        id: impl Into<String>,
        sink: Sink,
        mode: SinkMode,
        ctx: SessionContext,
        datalake: Arc<Datalake>,
    ) -> Self {
        let meta = NodeMeta::new(id).add_input_port(None);
        Self {
            meta,
            sink,
            mode,
            ctx,
            datalake,
        }
    }

    /// The destination this sink writes to.
    pub fn sink(&self) -> &Sink {
        &self.sink
    }

    /// Whether this sink appends to, or overwrites, the destination.
    pub fn mode(&self) -> SinkMode {
        self.mode
    }

    /// Return the rows already stored at `path` concatenated with `new`, used
    /// to implement true single-file append.
    ///
    /// DataFusion's single-file sink always replaces the target file, so an
    /// append is realized by reading the current contents back, casting each
    /// column to `new`'s schema (so the schemas line up for `union`), and
    /// emitting one combined [`DataFrame`] that is then written with
    /// [`InsertOp::Overwrite`]. If the destination does not yet exist, `new`
    /// is returned unchanged.
    async fn append_existing(
        &self,
        path: &str,
        format: WriteFormat,
        new: DataFrame,
    ) -> Result<DataFrame, SinkError> {
        use datafusion::logical_expr::cast;
        use datafusion::prelude::{CsvReadOptions, ParquetReadOptions, col};

        if !std::path::Path::new(path).exists() {
            return Ok(new);
        }

        let read_err = |e: datafusion::error::DataFusionError| SinkError::Write {
            path: path.to_string(),
            source: e,
        };
        let existing = match format {
            WriteFormat::Csv => self
                .ctx
                .read_csv(path, CsvReadOptions::default())
                .await
                .map_err(read_err)?,
            WriteFormat::Parquet => self
                .ctx
                .read_parquet(path, ParquetReadOptions::default())
                .await
                .map_err(read_err)?,
        };

        // Cast each existing column to the new DataFrame's field type so the
        // two schemas are union-compatible. This matters most for CSV, where
        // integers re-read back as `Int64` regardless of how they were
        // written.
        let target = new.schema().inner();
        let cast_exprs: Vec<_> = target
            .fields()
            .iter()
            .map(|f| cast(col(f.name()), f.data_type().clone()))
            .collect();
        let existing = existing.select(cast_exprs).map_err(read_err)?;
        existing.union(new).map_err(read_err)
    }

    /// Handle to the Iceberg data lake; used by the Iceberg write path
    /// once that branch is wired up.
    pub fn datalake(&self) -> Arc<Datalake> {
        self.datalake.clone()
    }
}

#[async_trait]
impl DagNode for SinkNode {
    fn meta(&self) -> &NodeMeta {
        &self.meta
    }

    fn clone_box(&self) -> Box<dyn DagNode> {
        let cp_node = Self {
            meta: self.meta.clone(),
            sink: self.sink.clone(),
            mode: self.mode,
            ctx: self.ctx.clone(),
            datalake: self.datalake.clone(),
        };

        Box::new(cp_node)
    }

    fn node_type(&self) -> &str {
        "sink"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn execute(&mut self, inputs: &[NodeInput]) -> Result<PortOutputs, DagError> {
        let input = inputs.first().ok_or(SinkError::InvalidInput {
            message: "SinkNode requires exactly one upstream input".to_string(),
        })?;

        match &self.sink {
            Sink::File { path, format } => {
                let path = normalize_path(path);
                let df = input.data.clone();

                // Resolve the DataFrame to actually write. DataFusion's
                // `write_csv`/`write_parquet` do not implement
                // `InsertOp::Overwrite` and their single-file sink always
                // *replaces* the target — so an overwrite is "drop the
                // existing file then write", and an append is "read the
                // existing rows back, merge them, then write".
                let to_write = match self.mode {
                    SinkMode::Overwrite => {
                        let _ = std::fs::remove_file(&path);
                        df
                    }
                    SinkMode::Append => self.append_existing(&path, *format, df).await?,
                };

                let options = DataFrameWriteOptions::new().with_single_file_output(true);

                let res = match format {
                    WriteFormat::Csv => {
                        to_write.write_csv(&path, options, None::<CsvOptions>).await
                    }
                    WriteFormat::Parquet => {
                        to_write
                            .write_parquet(&path, options, None::<TableParquetOptions>)
                            .await
                    }
                };
                res.map_err(|e| SinkError::Write {
                    path: path.clone(),
                    source: e,
                })?;
            }
            Sink::Iceberg { ident } => {
                // Write directly through iceberg-rs's native writer instead of
                // DataFusion's `INSERT INTO`. The upstream `DataFrame` is
                // collected into `RecordBatch`es and fed to an
                // `UnpartitionedWriter` backed by a Parquet rolling file writer;
                // the resulting data files are committed via a fast-append
                // transaction. Bypassing DataFusion avoids the multipart-upload
                // failure surfaced by the SQL write path.
                let df = input.data.clone();
                let datalake = self.datalake();

                // Parse `ident` ("ns1.ns2...table") into namespace + table name.
                let mut ns_vec: Vec<String> = ident.split('.').map(|e| e.to_string()).collect();
                let table_name = ns_vec.pop().ok_or(SinkError::Iceberg {
                    msg: "Illegal table ident - table name is empty".to_string(),
                })?;
                let namespace = NamespaceIdent::from_vec(ns_vec)
                    .map_err(|e| SinkError::Iceberg { msg: e.to_string() })?;

                // Ensure the table exists, deriving the Iceberg schema from the
                // incoming Arrow schema.
                let arrow_schema = df.schema().inner();
                let iceberg_schema = arrow_schema_to_schema_auto_assign_ids(arrow_schema)
                    .map_err(|e| SinkError::Iceberg { msg: e.to_string() })?;
                let table_creation = TableCreation::builder()
                    .name(table_name)
                    .schema(iceberg_schema)
                    .build();

                // The pinned iceberg-rs exposes no overwrite transaction; for
                // `Overwrite` we drop any pre-existing table so the write
                // starts from an empty table. `Append` leaves it in place.
                if matches!(self.mode, SinkMode::Overwrite) {
                    let catalog = datalake
                        .get_catalog()
                        .await
                        .map_err(|e| SinkError::Iceberg { msg: e.to_string() })?;
                    let table_ident =
                        iceberg::TableIdent::new(namespace.clone(), table_creation.name.clone());
                    // Best-effort: a missing table is expected on first write.
                    let _ = catalog.drop_table(&table_ident).await;
                }

                let table = datalake
                    .create_table_if_not_exist(&namespace, table_creation)
                    .await
                    .map_err(|e| SinkError::Iceberg { msg: e.to_string() })?;

                // Build the native iceberg writer stack. Match by field *name*
                // (not id) because the upstream Arrow batches carry no Iceberg
                // field-id metadata.
                let schema = table.metadata().current_schema().clone();
                let parquet_builder = ParquetWriterBuilder::new_with_match_mode(
                    WriterProperties::default(),
                    schema,
                    FieldMatchMode::Name,
                );
                let location_gen = DefaultLocationGenerator::new(table.metadata())
                    .map_err(|e| SinkError::Iceberg { msg: e.to_string() })?;
                let name_gen = DefaultFileNameGenerator::new(
                    "sink".to_string(),
                    None,
                    DataFileFormat::Parquet,
                );
                let rolling_builder = RollingFileWriterBuilder::new_with_default_file_size(
                    parquet_builder,
                    table.file_io().clone(),
                    location_gen,
                    name_gen,
                );
                let data_file_builder = DataFileWriterBuilder::new(rolling_builder);
                let mut writer = UnpartitionedWriter::new(data_file_builder);

                // Collect upstream batches and write them through the iceberg writer.
                let batches = df.collect().await.map_err(|e| SinkError::Write {
                    path: format!("iceberg://{ident}"),
                    source: e,
                })?;
                for batch in batches {
                    writer
                        .write(batch)
                        .await
                        .map_err(|e| SinkError::Iceberg { msg: e.to_string() })?;
                }

                // Close the writer to materialize data files, then commit them
                // to the table via a fast-append transaction.
                let data_files = writer
                    .close()
                    .await
                    .map_err(|e| SinkError::Iceberg { msg: e.to_string() })?;

                let catalog = datalake
                    .get_catalog()
                    .await
                    .map_err(|e| SinkError::Iceberg { msg: e.to_string() })?;
                let tx = Transaction::new(&table);
                let action = tx.fast_append().add_data_files(data_files);
                action
                    .apply(tx)
                    .map_err(|e| SinkError::Iceberg { msg: e.to_string() })?
                    .commit(catalog.as_ref())
                    .await
                    .map_err(|e| SinkError::Iceberg { msg: e.to_string() })?;
            }
        }
        Ok(HashMap::new())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow_array::{Int32Array, RecordBatch, StringArray};
    use arrow_schema::{DataType, Field, Schema};
    use datafusion::prelude::{DataFrame, SessionContext};
    use datalake::Datalake;

    use crate::data_engine::{
        Sink, SinkMode, SinkNode, WriteFormat,
        dag::{DagNode, NodeInput},
    };

    /// Build a small in-memory [`DataFrame`] for sink tests.
    ///
    /// Two columns, three rows — enough to round-trip through both CSV and
    /// Parquet writers without bloating the test runtime. Mirrors the helper
    /// style used in `sql_node::tests::setup_test_node`.
    #[allow(dead_code)]
    fn sample_dataframe() -> (SessionContext, DataFrame) {
        let ctx = SessionContext::new();
        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int32, false),
            Field::new("name", DataType::Utf8, false),
        ]));
        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(Int32Array::from(vec![1, 2, 3])),
                Arc::new(StringArray::from(vec!["alice", "bob", "carol"])),
            ],
        )
        .expect("sample RecordBatch should construct");
        let df = ctx
            .read_batch(batch)
            .expect("ctx should accept sample batch");
        (ctx, df)
    }

    #[tokio::test]
    async fn test_sink_iceberg() {
        let ctx = Datalake::default().get_ctx().await.unwrap();
        let datalake = Arc::new(Datalake::default());
        let mut node = SinkNode::new(
            "test_id",
            Sink::Iceberg {
                ident: "gwas.test1".to_string(),
            },
            crate::data_engine::nodes::sink::SinkMode::Overwrite,
            ctx,
            datalake,
        );

        let (_, df) = sample_dataframe();
        let input = NodeInput { port: 0, data: df };
        let _res = node.execute(&[input]).await.unwrap();
        // let df = res.get(&0).unwrap();
        // df.clone().show().await.unwrap();
    }

    /// A fresh DataFrame whose rows differ from [`sample_dataframe`] so that
    /// append vs. overwrite is distinguishable by reading the file back.
    fn second_dataframe() -> (SessionContext, DataFrame) {
        let ctx = SessionContext::new();
        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int32, false),
            Field::new("name", DataType::Utf8, false),
        ]));
        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(Int32Array::from(vec![4, 5])),
                Arc::new(StringArray::from(vec!["dave", "eve"])),
            ],
        )
        .expect("second RecordBatch should construct");
        let df = ctx
            .read_batch(batch)
            .expect("ctx should accept second batch");
        (ctx, df)
    }

    /// Read the `id` column of a CSV file back as a sorted `Vec<i32>`.
    ///
    /// DataFusion infers integer CSV columns as `Int64`, so we downcast to
    /// `Int64Array` regardless of how the value was originally typed.
    async fn read_csv_ids(ctx: &SessionContext, path: &str) -> Vec<i32> {
        use arrow_array::Int64Array;
        use datafusion::prelude::CsvReadOptions;
        let mut ids: Vec<i32> = ctx
            .read_csv(path, CsvReadOptions::default())
            .await
            .expect("read back sink output")
            .select(vec![datafusion::prelude::col("id")])
            .expect("select id")
            .collect()
            .await
            .expect("collect ids")
            .into_iter()
            .flat_map(|b| {
                b.column(0)
                    .as_any()
                    .downcast_ref::<Int64Array>()
                    .expect("id is Int64")
                    .iter()
                    .map(|v| v.expect("non-null id") as i32)
                    .collect::<Vec<_>>()
            })
            .collect();
        ids.sort();
        ids
    }

    /// `Overwrite` replaces the destination file entirely.
    #[tokio::test]
    async fn test_sink_file_overwrite_replaces() {
        let ctx = SessionContext::new();
        let datalake = Arc::new(Datalake::default());
        let path = format!("/tmp/sink_overwrite_{}.csv", std::process::id(),);

        let sink = |df: DataFrame, mode| {
            let mut node = SinkNode::new(
                "ow",
                Sink::File {
                    path: path.clone(),
                    format: WriteFormat::Csv,
                },
                mode,
                ctx.clone(),
                datalake.clone(),
            );
            async move { node.execute(&[NodeInput { port: 0, data: df }]).await }
        };

        sink(sample_dataframe().1, SinkMode::Overwrite)
            .await
            .unwrap();
        sink(second_dataframe().1, SinkMode::Overwrite)
            .await
            .unwrap();

        let ids = read_csv_ids(&ctx, &path).await;
        assert_eq!(ids, vec![4, 5], "overwrite must keep only the second write");
        let _ = std::fs::remove_file(&path);
    }

    /// `Append` stacks successive writes onto the destination file.
    #[tokio::test]
    async fn test_sink_file_append_accumulates() {
        let ctx = SessionContext::new();
        let datalake = Arc::new(Datalake::default());
        let path = format!("/tmp/sink_append_{}.csv", std::process::id());

        let write = |df: DataFrame| {
            let mut node = SinkNode::new(
                "ap",
                Sink::File {
                    path: path.clone(),
                    format: WriteFormat::Csv,
                },
                SinkMode::Append,
                ctx.clone(),
                datalake.clone(),
            );
            async move { node.execute(&[NodeInput { port: 0, data: df }]).await }
        };

        write(sample_dataframe().1).await.unwrap();
        write(second_dataframe().1).await.unwrap();

        let ids = read_csv_ids(&ctx, &path).await;
        assert_eq!(
            ids,
            vec![1, 2, 3, 4, 5],
            "append must keep rows from both writes"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// Regression: when an upstream SqlNode produces a `List(Utf8)` column
    /// (e.g. via `unnest` on a VCF Struct, or via `CAST(<struct> AS VARCHAR)`)
    /// and the downstream query references that column with `get_field(<col>,
    /// 'ES')`, DataFusion raises one of two known errors today, both
    /// originating in `datafusion-functions-53.x::getfield.rs`:
    ///
    /// 1. **Planning path** (`return_field_from_args:372`) →
    ///    `"Cannot access field at argument N: type List(Utf8) is not Struct, Map, or Null"`.
    /// 2. **Execution path** (`invoke_with_args:230`) → the agent's report:
    ///    `"Execution error: get_field is only possible on maps or structs.
    ///     Received List(Utf8) with Utf8("ES") index"`
    ///
    /// Both are routed through the same root cause (List ≠ Struct/Map). Until
    /// DataFusion unifies the two messages we accept either, but assert the
    /// common ingredients (`get_field`, `List(Utf8)`, `ES`). This pins the
    /// regression without making the test brittle to whichever planner pass
    /// short-circuits first.
    #[tokio::test]
    async fn test_get_field_on_list_raises_expected_error() {
        let ctx = SessionContext::new();

        // Build a table with a `List(Utf8)` column. The simplest
        // construction is to register a tiny RecordBatch with the arrow
        // ListArray.
        use arrow_array::{Array, ListArray};
        use arrow_buffer::OffsetBuffer;

        let item_field = Arc::new(Field::new("item", DataType::Utf8, true));
        let list_dt = DataType::List(item_field.clone());
        let schema = Arc::new(Schema::new(vec![Field::new(
            "tag_list",
            list_dt.clone(),
            false,
        )]));

        let values = StringArray::from(vec!["ES", "EZ", "AF"]);
        let offsets = OffsetBuffer::new(vec![0, 2, 3].into());
        let list_array = ListArray::try_new(
            item_field.clone(),
            offsets,
            Arc::new(values) as Arc<dyn Array>,
            None,
        )
        .expect("ListArray should construct");

        let batch = RecordBatch::try_new(schema, vec![Arc::new(list_array) as _])
            .expect("RecordBatch should construct");
        let df = ctx.read_batch(batch).expect("DF should wrap batch");
        ctx.register_table("upstream", df.clone().into_view())
            .expect("register view");

        // The SQL: get_field(<List column>, 'ES'). This is the exact call
        // shape the agent reported, modulo a different base-expression name.
        let result = ctx
            .sql("SELECT get_field(tag_list, 'ES') AS es_val FROM upstream")
            .await;

        let msg = match result {
            Ok(df) => {
                // Planning accepted — execution must surface the runtime form.
                let exec_err = df
                    .collect()
                    .await
                    .expect_err("expected runtime error from get_field on List");
                format!("{exec_err}")
            }
            Err(plan_err) => format!("{plan_err}"),
        };

        // Either path is acceptable today; the common fingerprint is what
        // proves the regression. If a future DataFusion splits this into two
        // different errors we'll fail loudly and decide which to keep.
        let get_field_runtime = msg.contains("get_field is only possible on maps or structs")
            && msg.contains("List(Utf8)")
            && msg.contains("ES");
        let get_field_planning = msg.contains("Cannot access field at argument")
            && msg.contains("List(Utf8)")
            && msg.contains("not Struct, Map, or Null");
        assert!(
            get_field_runtime || get_field_planning,
            "get_field(List, 'ES') must fail with one of the known get_field/\
             List/ES error shapes; got: {msg}"
        );
    }

    /// Regression: the actual failure mode the agent hit at sink time —
    /// `INSERT INTO <target> SELECT * FROM <source>` where the target's
    /// column type is `Struct(ES: Float64, …)` and the source view's
    /// position-aligned column is `List(Utf8)`. The Iceberg sink exercises
    /// exactly this shape.
    ///
    /// We don't need a real Iceberg catalog: a plain `Memory` table with a
    /// Struct-typed column plays the same role from the planner's point of
    /// view, because `INSERT INTO` only looks at the registered
    /// `TableProvider`'s schema, not at its backing store.
    #[tokio::test]
    async fn test_insert_into_struct_with_list_source_errors_at_execution() {
        use arrow_array::ListArray;
        use arrow_buffer::OffsetBuffer;
        use datafusion::arrow::datatypes::Fields;

        let ctx = SessionContext::new();

        // Source view: one row with a List(Utf8) column named `info` (matching
        // oxbow's naming so the failure story is the same as for a VCF).
        let item_field = Arc::new(Field::new("item", DataType::Utf8, true));
        let source_schema = Arc::new(Schema::new(vec![Field::new(
            "info",
            DataType::List(item_field.clone()),
            true,
        )]));
        let values = StringArray::from(vec!["ES", "EZ"]);
        let offsets = OffsetBuffer::new(vec![0, 1, 2].into());
        let list_array = ListArray::try_new(
            item_field.clone(),
            offsets,
            Arc::new(values) as Arc<dyn arrow_array::Array>,
            None,
        )
        .expect("ListArray should construct");
        let source_batch =
            RecordBatch::try_new(source_schema.clone(), vec![Arc::new(list_array) as _])
                .expect("source batch should build");
        let source_df = ctx.read_batch(source_batch).expect("read_batch");
        ctx.register_table("source_view", source_df.into_view())
            .expect("register source view");

        // Target memory table: position-aligned `info` column declared as a
        // Struct(ES: Float64) — the same shape an iceberg table would expose
        // after the agent's `arrow_schema_to_schema_auto_assign_ids` step if
        // upstream somehow returned a List-typed info column.
        let target_schema = Arc::new(Schema::new(vec![Field::new(
            "info",
            DataType::Struct(Fields::from(vec![Field::new(
                "ES",
                DataType::Float64,
                true,
            )])),
            true,
        )]));
        ctx.register_csv(
            "ignored_csv",
            "test_datasets/Iris.csv",
            datafusion::prelude::CsvReadOptions::default(),
        )
        .await
        .ok(); // best-effort; target memory table doesn't depend on it

        // Register an empty in-memory table with the target schema. We do
        // that via a RecordBatch containing zero rows but the desired schema.
        let empty_batch = RecordBatch::new_empty(target_schema.clone());
        let target_df = ctx.read_batch(empty_batch).expect("target df");
        ctx.register_table("target_table", target_df.into_view())
            .expect("register target table");

        let result = ctx
            .sql("INSERT INTO target_table SELECT * FROM source_view")
            .await;

        let err = match result {
            Ok(df) => {
                // If the planner accepts, execution should fail because
                // List→Struct is a structurally impossible cast.
                let exec = df.collect().await;
                exec.expect_err("expected execution to fail on List→Struct")
            }
            Err(e) => e,
        };

        let msg = format!("{err}");

        // The agent's obstacle #3 surfaced as
        // `get_field is only possible … List(Utf8) with Utf8("ES") index`.
        // For the bare INSERT case (no explicit get_field in the source SQL)
        // the planner surfaces one of several known messages — all routed
        // through the same root cause ("source column is List, target is
        // Struct"). Pin any of the known shapes so we don't have to chase
        // DataFusion's exact wording per release.
        let get_field_route = msg.contains("get_field is only possible")
            && msg.contains("List(Utf8)")
            && msg.contains("ES");
        let cast_route = msg.contains("Cannot cast column of type")
            || msg.contains("to struct")
            || msg.contains("Source must be a struct to cast to struct")
            || msg.contains("schema mismatch");
        let conversion_route = msg.contains("Cannot automatically convert")
            && msg.contains("List(Utf8)")
            && msg.contains("Struct");
        assert!(
            get_field_route || cast_route || conversion_route,
            "INSERT should fail with get_field/List/ES, cast-to-struct, or \
             Cannot automatically convert List→Struct; got: {msg}"
        );
    }
}
