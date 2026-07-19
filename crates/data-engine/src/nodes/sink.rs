//! Unified sink node: consumes an upstream `DataFrame` and writes it out.
//!
//! Symmetric to [`crate::nodes::SourceNode`]: a [`SinkNode`] has
//! exactly one input and produces no output. The destination is described by
//! [`Sink`] — a file (CSV / Parquet) or an Iceberg table.

use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;
use datafusion::common::HashMap;
use datafusion::common::config::{CsvOptions, TableParquetOptions};
use datafusion::dataframe::{DataFrame, DataFrameWriteOptions};
use datafusion::prelude::SessionContext;
use datalake::Datalake;
use iceberg::arrow::arrow_schema_to_schema_auto_assign_ids;
use iceberg::{Catalog, NamespaceIdent, TableCreation, TableIdent};
use schemars::{schema_for, JsonSchema};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::meta::{DagNode, NodeInput, NodePorts};
use super::source::normalize_path;
use crate::{
    dag::DagError,
    dag::graph::PortOutputs,
    node_registry::registry::{NodeCtx, NodeFactory},
};

/// Where a [`SinkNode`] writes to.
#[derive(Debug, Clone)]
pub enum Sink {
    /// Write to a file path or URL.
    File { path: String, format: WriteFormat },
    /// Write to an Iceberg table (catalog write path must be available).
    Iceberg { ident: String },
}

/// Supported on-disk write formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum WriteFormat {
    Csv,
    Parquet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
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
    meta: NodePorts,
    sink: Sink,
    mode: SinkMode,
    ctx: SessionContext,
    datalake: Arc<Datalake>,
}

// ─────────────────────────────────────────────────────────────────────
// WORKAROUND(iceberg-rust): bare reserved column names.
//
// iceberg-rust reserves the *bare* names `pos` and `file_path` (no leading
// underscore) as metadata-column names (`RESERVED_COL_NAME_DELETE_FILE_POS`
// / `RESERVED_COL_NAME_DELETE_FILE_PATH`). During a data-table scan it
// resolves projected column names with metadata-first precedence and no
// fallback to a same-named data column, so a real data column named `pos`/
// `file_path` is shadowed and reads back as
// `External(Unexpected => "field not found")`.
//
// Unlike the spec's `_`-prefixed metadata names (`_file`, `_pos`, …), these
// bare names are NOT reserved by the Iceberg spec, so they collide with
// legitimate user columns — notably VCF `pos` (oxbow emits it as `pos`).
//
// Repro + workaround tests: `tests/replicate_pos_field_not_found.rs`.
// Upstream issue: apache/iceberg-rust (data column `pos`/`file_path`
// shadowed by reserved metadata name).
//
// >>> Remove `ICEBERG_RESERVED_BARE_NAMES` and `rename_iceberg_reserved_columns`
// >>> (and the call site in `execute`'s `Sink::Iceberg` arm) once upstream
// >>> fixes the name clash.
// ─────────────────────────────────────────────────────────────────────

/// Top-level column names iceberg-rust reserves *without* a `_` prefix and
/// that therefore collide with user data columns. Mirror of the non-underscore
/// `RESERVED_COL_NAME_*` constants in iceberg-rust's `metadata_columns.rs`.
const ICEBERG_RESERVED_BARE_NAMES: &[&str] = &["pos", "file_path"];

/// Rename any top-level column whose name is a bare iceberg-rust reserved
/// metadata-column name, appending a `_col` suffix (extra `_` until free if
/// that collides). Only top-level columns are affected: the scan bug is on
/// projected column names, and nested struct fields are reached by path
/// (e.g. `info.pos`), not as a scan projection. Emits a `tracing::warn!` per
/// rename so callers know the read-back schema differs from what they wrote.
fn rename_iceberg_reserved_columns(mut df: DataFrame) -> DataFrame {
    let mut names: HashSet<String> = df
        .schema()
        .fields()
        .iter()
        .map(|f| f.name().clone())
        .collect();

    for &reserved in ICEBERG_RESERVED_BARE_NAMES {
        if names.contains(reserved) {
            let new = unique_column_name(reserved, &names);
            tracing::warn!(
                column = reserved,
                renamed_to = %new,
                "iceberg-rust reserves the bare column name `{reserved}` as a metadata \
                 column, which would make it unreadable after the Iceberg round-trip; \
                 renaming to `{new}`"
            );
            df = df
                .with_column_renamed(reserved, new.as_str())
                .expect("renaming an existing top-level column must succeed");
            names.remove(reserved);
            names.insert(new);
        }
    }
    df
}

/// Return `"{base}_col"`, appending extra `_` until it does not collide with
/// any name in `taken`.
fn unique_column_name(base: &str, taken: &HashSet<String>) -> String {
    let mut candidate = format!("{base}_col");
    while taken.contains(&candidate) {
        candidate.push('_');
    }
    candidate
}

impl SinkNode {
    pub fn new(
        sink: Sink,
        mode: SinkMode,
        ctx: SessionContext,
        datalake: Arc<Datalake>,
    ) -> Self {
        Self {
            meta: port_layout(),
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

#[derive(Debug, JsonSchema, Deserialize)]
#[serde(tag = "type")]
pub enum SinkNodeSpec {
    #[serde(rename = "file")]
    File {
        path: String,
        format: WriteFormat,
        #[serde(default)]
        mode: SinkMode,
    },
    #[serde(rename = "iceberg")]
    Iceberg {
        ident: String,
        #[serde(default)]
        mode: SinkMode,
    },
}

pub struct SinkNodeFactory {}

/// Static port layout for every [`SinkNode`]: a single untyped input port and
/// no outputs.
fn port_layout() -> NodePorts {
    NodePorts::new().add_input_port(None)
}

impl NodeFactory for SinkNodeFactory {
    fn kind(&self) -> &'static str {
        "sink"
    }

    fn spec_schema(&self) -> schemars::Schema {
        schema_for!(SinkNodeSpec)
    }

    fn ports(&self) -> NodePorts {
        port_layout()
    }

    fn build(
        &self,
        spec: serde_json::Value,
        node_ctx: NodeCtx,
    ) -> crate::node_registry::error::Result<Box<dyn DagNode>> {
        let node_spec: SinkNodeSpec = serde_json::from_value(spec)?;
        let (sink, mode) = match node_spec {
            SinkNodeSpec::File { path, format, mode } => {
                (Sink::File { path, format }, mode)
            }
            SinkNodeSpec::Iceberg { ident, mode } => {
                (Sink::Iceberg { ident }, mode)
            }
        };
        let node = SinkNode::new(sink, mode, node_ctx.session, node_ctx.datalake);
        Ok(Box::new(node))
    }
}

#[async_trait]
impl DagNode for SinkNode {
    fn ports(&self) -> &NodePorts {
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

    fn kind(&self) -> &'static str {
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
                // Write through DataFusion's `INSERT INTO` which delegates to the
                // registered `IcebergCatalogProvider` (handles parquet writing,
                // file-I/O, and the append commit).
                //
                // Steps:
                // 1. Parse `ident` ("ns1.ns2...table") into namespace + table name.
                // 2. Derive the Iceberg schema from the incoming Arrow schema.
                // 3. For `Overwrite`: drop the table, recreate it empty.
                //    For `Append`:  create the table if it does not exist.
                // 4. Register the upstream DataFrame as a temp view, then
                //    `INSERT INTO iceberg.<ns>.<table> SELECT * FROM <view>`.
                let df = input.data.clone();
                // WORKAROUND(iceberg-rust): rename bare reserved metadata
                // column names (`pos`, `file_path`) so they survive the
                // round-trip. Remove once upstream fixes the clash.
                let df = rename_iceberg_reserved_columns(df);
                let datalake = self.datalake();

                // 1. Parse ident.
                let mut ns_vec: Vec<String> = ident.split('.').map(|e| e.to_string()).collect();
                let table_name = ns_vec.pop().ok_or(SinkError::Iceberg {
                    msg: "Illegal table ident - table name is empty".to_string(),
                })?;
                let namespace = NamespaceIdent::from_vec(ns_vec)
                    .map_err(|e| SinkError::Iceberg { msg: e.to_string() })?;

                // 2. Derive Iceberg schema from the upstream Arrow schema.
                let arrow_schema = df.schema().inner();
                let iceberg_schema = arrow_schema_to_schema_auto_assign_ids(arrow_schema)
                    .map_err(|e| SinkError::Iceberg { msg: e.to_string() })?;

                // 3. Ensure the table exists with the correct (empty) state.
                let catalog = datalake
                    .get_catalog()
                    .await
                    .map_err(|e| SinkError::Iceberg { msg: e.to_string() })?;

                let table_ident = TableIdent::new(namespace.clone(), table_name.clone());
                match self.mode {
                    SinkMode::Overwrite => {
                        // Drop any pre-existing table so the write starts from
                        // an empty table. Best-effort: a missing table is
                        // expected on first write.
                        let _ = catalog.drop_table(&table_ident).await;
                        let creation = TableCreation::builder()
                            .name(table_name.clone())
                            .schema(iceberg_schema)
                            .build();
                        datalake
                            .create_table_if_not_exist(&namespace, creation)
                            .await
                            .map_err(|e| SinkError::Iceberg { msg: e.to_string() })?;
                    }
                    SinkMode::Append => {
                        // If the table already exists with data, skip creation
                        // — the INSERT will append rows.
                        if !catalog
                            .table_exists(&table_ident)
                            .await
                            .map_err(|e| SinkError::Iceberg { msg: e.to_string() })?
                        {
                            let creation = TableCreation::builder()
                                .name(table_name.clone())
                                .schema(iceberg_schema)
                                .build();
                            datalake
                                .create_table_if_not_exist(&namespace, creation)
                                .await
                                .map_err(|e| SinkError::Iceberg { msg: e.to_string() })?;
                        }
                    }
                }

                // Re-register the iceberg catalog so the DataFusion planner
                // discovers the table we just created through the REST API.
                // The previous provider cached its table list at creation time,
                // so a freshly-created table is invisible to it.
                let fresh_provider = datalake
                    .get_provider()
                    .await
                    .map_err(|e| SinkError::Iceberg { msg: e.to_string() })?;
                self.ctx
                    .register_catalog("iceberg", Arc::new(fresh_provider));

                // 4. Register the upstream DataFrame as a temp view and INSERT.
                let src_name = format!("__sink_src_{:x}", std::process::id());
                let _ = self.ctx.deregister_table(&src_name);
                let view = df.into_view();
                self.ctx
                    .register_table(&src_name, view)
                    .map_err(|e| SinkError::Write {
                        path: format!("iceberg://{ident}"),
                        source: e,
                    })?;

                // Build a fully-qualified `iceberg.<ns>.<table>` reference.
                // DataFusion's SQL parser handles multi-part identifiers with
                // up to 4 segments, which covers typical nested namespaces.
                let mut parts = vec!["iceberg".to_string()];
                parts.extend(namespace.inner().iter().cloned());
                parts.push(table_name);
                let fqn = parts.join(".");

                let sql = format!("INSERT INTO {fqn} SELECT * FROM {src_name}");
                self.ctx
                    .sql(&sql)
                    .await
                    .map_err(|e| SinkError::Write {
                        path: format!("iceberg://{ident}"),
                        source: e,
                    })?
                    .collect()
                    .await
                    .map_err(|e| SinkError::Write {
                        path: format!("iceberg://{ident}"),
                        source: e,
                    })?;

                self.ctx.deregister_table(&src_name).ok();
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

    use crate::nodes::{
        Sink, SinkMode, SinkNode, WriteFormat,
        meta::{DagNode, NodeInput},
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

    // ── WORKAROUND(iceberg-rust) tests: remove with the workaround ──

    /// `rename_iceberg_reserved_columns` rewrites top-level `pos`/`file_path`
    /// to a non-reserved name, leaves other columns untouched, and preserves
    /// data.
    #[tokio::test]
    async fn test_rename_iceberg_reserved_columns() {
        use super::{ICEBERG_RESERVED_BARE_NAMES, rename_iceberg_reserved_columns};

        let ctx = SessionContext::new();
        let schema = Arc::new(Schema::new(vec![
            Field::new("chrom", DataType::Utf8, false),
            Field::new("pos", DataType::Int32, false),
            Field::new("file_path", DataType::Utf8, false),
        ]));
        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(StringArray::from(vec!["1", "2", "3"])),
                Arc::new(Int32Array::from(vec![10, 20, 30])),
                Arc::new(StringArray::from(vec!["a", "b", "c"])),
            ],
        )
        .unwrap();
        let df = ctx.read_batch(batch).unwrap();

        let renamed = rename_iceberg_reserved_columns(df);
        let names: Vec<String> = renamed
            .schema()
            .fields()
            .iter()
            .map(|f| f.name().clone())
            .collect();

        // Reserved bare names are gone, replaced by `<name>_col`.
        assert!(!names.contains(&"pos".to_string()));
        assert!(!names.contains(&"file_path".to_string()));
        assert!(names.contains(&"pos_col".to_string()));
        assert!(names.contains(&"file_path_col".to_string()));
        // Non-reserved columns are untouched.
        assert!(names.contains(&"chrom".to_string()));
        // No reserved bare name survives.
        for reserved in ICEBERG_RESERVED_BARE_NAMES {
            assert!(!names.contains(&(**reserved).to_string()));
        }

        // Data is preserved under the new names.
        let batches = renamed.collect().await.unwrap();
        let pos_col = batches[0].column_by_name("pos_col").unwrap();
        let vals: Vec<i32> = pos_col
            .as_any()
            .downcast_ref::<Int32Array>()
            .unwrap()
            .values()
            .iter()
            .copied()
            .collect();
        assert_eq!(vals, vec![10, 20, 30]);
    }

    /// Suffix collision is resolved by appending extra `_`.
    #[test]
    fn test_unique_column_name_avoids_collision() {
        use super::unique_column_name;
        use std::collections::HashSet;
        let taken: HashSet<String> = ["pos_col", "pos"].iter().map(|s| s.to_string()).collect();
        // `pos_col` is taken → must extend further.
        let new = unique_column_name("pos", &taken);
        assert!(!taken.contains(&new));
        assert!(new.starts_with("pos_col"));
    }

    /// A schema with no reserved names passes through unchanged.
    #[tokio::test]
    async fn test_rename_noop_when_no_reserved() {
        use super::rename_iceberg_reserved_columns;

        let ctx = SessionContext::new();
        let schema = Arc::new(Schema::new(vec![
            Field::new("chrom", DataType::Utf8, false),
            Field::new("position", DataType::Int32, false),
        ]));
        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(StringArray::from(vec!["1", "2"])),
                Arc::new(Int32Array::from(vec![100, 200])),
            ],
        )
        .unwrap();
        let df = ctx.read_batch(batch).unwrap();

        let renamed = rename_iceberg_reserved_columns(df);
        let names: Vec<String> = renamed
            .schema()
            .fields()
            .iter()
            .map(|f| f.name().clone())
            .collect();
        assert_eq!(names, vec!["chrom", "position"]);
    }

    #[tokio::test]
    #[ignore]
    async fn test_sink_iceberg() {
        let ctx = Datalake::default().get_ctx().await.unwrap();
        let datalake = Arc::new(Datalake::default());
        let mut node = SinkNode::new(
            Sink::Iceberg {
                ident: "gwas.test4".to_string(),
            },
            crate::nodes::sink::SinkMode::Overwrite,
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
