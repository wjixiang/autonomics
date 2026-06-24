//! Core dataset model for in-memory analytical data.
//!
//! [`AetherDataset`] is an immutable, schema-aware, partitioned columnar
//! container analogous to Spark's **RDD** — each transformation produces a
//! new dataset (immutability), data is split across `RecordBatch` partitions
//! (parallelism-ready), and the full Arrow type system is exposed through
//! column extraction (bridging to `stat-primitives`).
//!
//! # Design principles (inspired by Spark RDD)
//!
//! | RDD property            | AetherDataset equivalent                   |
//! |-------------------------|--------------------------------------------|
//! | Immutable               | Every transform returns a new instance       |
//! | Partitioned             | `Vec<RecordBatch>` = logical partitions     |
//! | Typed                   | Arrow `SchemaRef` enforces column types      |
//! | Transformations (lazy)  | `select()`, `filter()`, `sort()` …          |
//! | Actions (eager)         | `collect()`, `count()`, `extract_f64()` …   |
//! | Lineage                 | `provenance` tracks derivation chain         |

pub mod store;

use std::fmt;
use std::sync::Arc;

use arrow_array::builder::{BooleanBufferBuilder, Float64Builder};
use arrow_array::{Array, BooleanArray, Float32Array, Float64Array, RecordBatch, StringArray};
use arrow_ord::sort::lexsort_to_indices;
use arrow_schema::{DataType, Field, Schema, SchemaRef, SortOptions};
use arrow_select::filter::filter_record_batch;
use arrow_select::take::take_record_batch;
use serde::{Deserialize, Serialize};

use crate::error::DatasetError;

// ---------------------------------------------------------------------------
// NullPolicy
// ---------------------------------------------------------------------------

/// Policy for handling null values when extracting numeric columns.
#[derive(Debug, Clone, Copy, Default)]
pub enum NullPolicy {
    /// Drop null values, returning only non-null elements.
    #[default]
    DropNulls,
    /// Replace nulls with the given fill value.
    Fill(f64),
    /// Return an error if any null is present.
    Reject,
}

// ---------------------------------------------------------------------------
// Provenance — lightweight lineage tracking
// ---------------------------------------------------------------------------

/// Describes how a dataset was derived.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Provenance {
    /// Created from a SQL query against an external source.
    Query { sql: String },
    /// Created by loading a table from the Iceberg catalog.
    Table {
        /// `"namespace.table"` identifier.
        table: String,
    },
    /// Derived from a transformation on one or more parent datasets.
    Transform {
        /// Human-readable operation name, e.g. `"select"`, `"filter"`, `"join"`.
        op: String,
        /// Names of the parent datasets that were inputs.
        parents: Vec<String>,
    },
    /// Created manually (e.g. from raw `RecordBatch` values).
    Manual,
}

impl fmt::Display for Provenance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Query { sql } => write!(f, "query: {sql}"),
            Self::Table { table } => write!(f, "table: {table}"),
            Self::Transform { op, parents } => write!(f, "{op}({})", parents.join(", ")),
            Self::Manual => f.write_str("manual"),
        }
    }
}

// ---------------------------------------------------------------------------
// AetherDataset — the core model
// ---------------------------------------------------------------------------

/// An in-memory, schema-aware, partitioned columnar dataset.
///
/// This is the central data abstraction for the agent analytics pipeline —
/// analogous to Spark's `Dataset[Row]` after `.collect()`. Each instance is
/// **immutable**; transformations return a new `AetherDataset`.
///
/// # Partitioning
///
/// Data is stored as a `Vec<RecordBatch>` where each `RecordBatch` is a
/// logical partition (row group). This mirrors Spark's RDD partition model
/// and enables future parallel processing via `rayon`.
///
/// # Type layout
///
/// ```text
/// AetherDataset {
///     name:       "gwas_significant"
///     schema:     [snp: Utf8, beta: Float64, se: Float64, p_value: Float64]
///     batches:    [RecordBatch(0..999), RecordBatch(1000..1999), ...]
///     provenance: Transform { op: "filter", parents: ["gwas_raw"] }
/// }
/// ```
#[derive(Clone)]
pub struct AetherDataset {
    name: String,
    schema: SchemaRef,
    batches: Vec<RecordBatch>,
    provenance: Provenance,
}

impl AetherDataset {
    // -----------------------------------------------------------------------
    // Constructors
    // -----------------------------------------------------------------------

    /// Create a dataset from a vector of `RecordBatch` partitions.
    ///
    /// The schema is inferred from the first non-empty batch. All batches
    /// must share the same schema (enforced by Arrow internally).
    ///
    /// # Errors
    ///
    /// Returns `DatasetError::Build` if `batches` is empty and no schema
    /// can be inferred.
    pub fn new(name: impl Into<String>, batches: Vec<RecordBatch>) -> Result<Self, DatasetError> {
        let schema = batches
            .iter()
            .find(|b| b.num_rows() > 0)
            .map(|b| b.schema())
            .ok_or_else(|| DatasetError::Build {
                message: "cannot infer schema from empty batches".into(),
            })?;

        Ok(Self {
            name: name.into(),
            schema,
            batches,
            provenance: Provenance::Manual,
        })
    }

    /// Create a dataset with an explicit schema (may have zero rows).
    pub fn with_schema(
        name: impl Into<String>,
        schema: SchemaRef,
        batches: Vec<RecordBatch>,
    ) -> Self {
        Self {
            name: name.into(),
            schema,
            batches,
            provenance: Provenance::Manual,
        }
    }

    /// Create an empty dataset with the given schema and no partitions.
    pub fn empty(name: impl Into<String>, schema: SchemaRef) -> Self {
        Self::with_schema(name, schema, Vec::new())
    }

    // -----------------------------------------------------------------------
    // Metadata accessors (actions — no transformation)
    // -----------------------------------------------------------------------

    /// Dataset name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Arrow schema.
    pub fn schema(&self) -> &SchemaRef {
        &self.schema
    }

    /// Total row count across all partitions.
    pub fn row_count(&self) -> usize {
        self.batches.iter().map(|b| b.num_rows()).sum()
    }

    /// Column count.
    pub fn column_count(&self) -> usize {
        self.schema.fields().len()
    }

    /// Number of `RecordBatch` partitions.
    pub fn num_partitions(&self) -> usize {
        self.batches.len()
    }

    /// Read-only access to the underlying partitions.
    pub fn batches(&self) -> &[RecordBatch] {
        &self.batches
    }

    /// Provenance information describing how this dataset was derived.
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    /// Iterate over column names.
    pub fn column_names(&self) -> impl Iterator<Item = &str> {
        self.schema.fields().iter().map(|f| f.name().as_str())
    }

    /// Get the [`Field`] for a column, if it exists.
    pub fn field(&self, name: &str) -> Option<&Field> {
        self.schema.field_with_name(name).ok()
    }

    /// Check whether a column exists.
    pub fn has_column(&self, name: &str) -> bool {
        self.schema.index_of(name).is_ok()
    }

    /// Column index (0-based).
    pub fn column_index(&self, name: &str) -> Option<usize> {
        self.schema.index_of(name).ok()
    }

    /// Schema summary as a JSON-serializable value.
    pub fn schema_json(&self) -> serde_json::Value {
        let columns: Vec<serde_json::Value> = self
            .schema
            .fields()
            .iter()
            .map(|f| {
                serde_json::json!({
                    "name": f.name(),
                    "type": f.data_type().to_string(),
                    "nullable": f.is_nullable(),
                })
            })
            .collect();

        serde_json::json!({
            "name": self.name,
            "row_count": self.row_count(),
            "column_count": self.column_count(),
            "columns": columns,
        })
    }

    /// Whether the dataset contains zero rows.
    pub fn is_empty(&self) -> bool {
        self.row_count() == 0
    }

    // -----------------------------------------------------------------------
    // Column extraction — bridge to stat-primitives
    // -----------------------------------------------------------------------

    /// Extract a numeric column as `Vec<f64>`.
    ///
    /// This is the primary bridge between Arrow's columnar format and
    /// `stat-primitives`'s `&[f64]` interface. Supports:
    ///
    /// | Arrow type          | Behaviour                              |
    /// |---------------------|----------------------------------------|
    /// | `Float64`           | Zero-copy via `values()`                |
    /// | `Float32` / `Int*`  | Cast to `Float64` then extract          |
    /// | Other               | Returns `DatasetError::NotNumeric`      |
    ///
    /// Null handling is governed by `null_policy`.
    pub fn extract_f64(
        &self,
        column: &str,
        null_policy: NullPolicy,
    ) -> Result<Vec<f64>, DatasetError> {
        let idx = self
            .column_index(column)
            .ok_or_else(|| DatasetError::ColumnNotFound {
                column: column.to_owned(),
                dataset: self.name.clone(),
            })?;

        let field = self.schema.field(idx);
        if !is_numeric_type(field.data_type()) {
            return Err(DatasetError::NotNumeric {
                column: column.to_owned(),
                actual: field.data_type().to_string(),
            });
        }

        // Fast path: Float64 is already in the right format.
        if field.data_type() == &DataType::Float64 {
            return self.extract_float64_column(idx, null_policy);
        }

        // General path: cast via Arrow compute, then extract.
        use arrow::compute::kernels::cast::cast;

        let mut result = Vec::with_capacity(self.row_count());
        for batch in &self.batches {
            let casted = cast(batch.column(idx), &DataType::Float64)?;
            let arr = casted
                .as_any()
                .downcast_ref::<Float64Array>()
                .expect("cast to Float64 must succeed");
            collect_f64(arr, &mut result, column, null_policy)?;
        }
        Ok(result)
    }

    /// Extract a Float32 column as a contiguous `Vec<f32>`.
    ///
    /// Returns an error if the column is not `Float32`.
    pub fn extract_f32(&self, column: &str) -> Result<Vec<f32>, DatasetError> {
        let idx = self
            .column_index(column)
            .ok_or_else(|| DatasetError::ColumnNotFound {
                column: column.to_owned(),
                dataset: self.name.clone(),
            })?;

        let field = self.schema.field(idx);
        if field.data_type() != &DataType::Float32 {
            return Err(DatasetError::NotNumeric {
                column: column.to_owned(),
                actual: field.data_type().to_string(),
            });
        }

        let mut result = Vec::with_capacity(self.row_count());
        for batch in &self.batches {
            let arr = batch
                .column(idx)
                .as_any()
                .downcast_ref::<Float32Array>()
                .unwrap();
            result.extend_from_slice(arr.values());
        }
        Ok(result)
    }

    /// Extract a string column as `Vec<Option<String>>`.
    ///
    /// Returns an error if the column is not `Utf8` or `LargeUtf8`.
    pub fn extract_string(&self, column: &str) -> Result<Vec<Option<String>>, DatasetError> {
        let idx = self
            .column_index(column)
            .ok_or_else(|| DatasetError::ColumnNotFound {
                column: column.to_owned(),
                dataset: self.name.clone(),
            })?;

        let field = self.schema.field(idx);
        match field.data_type() {
            DataType::Utf8 | DataType::LargeUtf8 => {}
            other => {
                return Err(DatasetError::NotNumeric {
                    column: column.to_owned(),
                    actual: other.to_string(),
                });
            }
        }

        let mut result = Vec::with_capacity(self.row_count());
        for batch in &self.batches {
            let arr = batch
                .column(idx)
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            for v in arr.iter() {
                result.push(v.map(|s| s.to_owned()));
            }
        }
        Ok(result)
    }

    /// Extract multiple columns as parallel `Vec<f64>` column vectors.
    ///
    /// This is the interface needed by `stat_primitives::regression::ols`
    /// which takes `predictors: &[&[f64]]` and `y: &[f64]`.
    ///
    /// All columns must have the same non-null row count after applying
    /// `null_policy`.
    pub fn extract_f64_columns(
        &self,
        columns: &[&str],
        null_policy: NullPolicy,
    ) -> Result<Vec<Vec<f64>>, DatasetError> {
        columns
            .iter()
            .map(|col| self.extract_f64(col, null_policy))
            .collect()
    }

    // -----------------------------------------------------------------------
    // Transformations (return a new AetherDataset — immutability)
    // -----------------------------------------------------------------------

    /// Project to a subset of columns.
    ///
    /// Equivalent to Spark's `rdd.map(row => Row(row("a"), row("b")))`
    /// but operates at the column level for efficiency.
    pub fn select(&self, columns: &[&str]) -> Result<Self, DatasetError> {
        let indices: Vec<usize> = columns
            .iter()
            .map(|c| {
                self.column_index(c)
                    .ok_or_else(|| DatasetError::ColumnNotFound {
                        column: c.to_string(),
                        dataset: self.name.clone(),
                    })
            })
            .collect::<Result<_, _>>()?;

        let new_fields: Vec<Field> = indices
            .iter()
            .map(|&i| self.schema.field(i).clone())
            .collect();
        let new_schema = Arc::new(Schema::new(new_fields));

        let new_batches: Vec<RecordBatch> = self
            .batches
            .iter()
            .map(|b| b.project(&indices))
            .collect::<Result<_, _>>()
            .map_err(|e| DatasetError::Arrow(e))?;

        let parents: Vec<String> = columns.iter().map(|s| s.to_string()).collect();
        Ok(AetherDataset::with_schema_and_provenance(
            self.name.clone(),
            new_schema,
            new_batches,
            Provenance::Transform {
                op: "select".into(),
                parents,
            },
        ))
    }

    /// Drop named columns.
    pub fn drop_columns(&self, columns: &[&str]) -> Result<Self, DatasetError> {
        let drop_set: std::collections::HashSet<&str> = columns.iter().copied().collect();
        let keep: Vec<&str> = self
            .column_names()
            .filter(|c| !drop_set.contains(c))
            .collect();
        self.select(&keep)
    }

    /// Rename a column.
    pub fn rename_column(&self, old: &str, new: &str) -> Result<Self, DatasetError> {
        let idx = self
            .column_index(old)
            .ok_or_else(|| DatasetError::ColumnNotFound {
                column: old.to_string(),
                dataset: self.name.clone(),
            })?;

        let mut new_fields: Vec<Field> =
            self.schema.fields().iter().map(|f| (**f).clone()).collect();
        new_fields[idx] = Field::new(
            new,
            new_fields[idx].data_type().clone(),
            new_fields[idx].is_nullable(),
        );
        let new_schema = Arc::new(Schema::new(new_fields));

        Ok(AetherDataset::with_schema_and_provenance(
            self.name.clone(),
            new_schema,
            self.batches.clone(),
            Provenance::Transform {
                op: format!("rename_column({old}→{new})"),
                parents: vec![self.name.clone()],
            },
        ))
    }

    /// Limit the dataset to the first `n` rows.
    pub fn limit(&self, n: usize) -> Self {
        let mut remaining = n;
        let mut new_batches = Vec::new();

        for batch in &self.batches {
            if remaining == 0 {
                break;
            }
            let take = remaining.min(batch.num_rows());
            remaining -= take;
            new_batches.push(batch.slice(0, take));
        }

        AetherDataset::with_schema_and_provenance(
            self.name.clone(),
            self.schema.clone(),
            new_batches,
            Provenance::Transform {
                op: format!("limit({n})"),
                parents: vec![self.name.clone()],
            },
        )
    }

    /// Concatenate two datasets (equivalent to `UNION ALL`).
    ///
    /// Both datasets must have compatible schemas (same column names and types).
    pub fn union(&self, other: &Self) -> Result<Self, DatasetError> {
        let self_names: Vec<&str> = self.column_names().collect();
        let other_names: Vec<&str> = other.column_names().collect();
        if self_names != other_names {
            return Err(DatasetError::Build {
                message: format!(
                    "schema mismatch: self columns [{}] vs other columns [{}]",
                    self_names.join(", "),
                    other_names.join(", "),
                ),
            });
        }

        let mut all_batches = self.batches.clone();
        all_batches.extend(other.batches.clone());

        Ok(AetherDataset::with_schema_and_provenance(
            self.name.clone(),
            self.schema.clone(),
            all_batches,
            Provenance::Transform {
                op: "union".into(),
                parents: vec![self.name.clone(), other.name.clone()],
            },
        ))
    }

    /// Sort by one or more columns.
    ///
    /// Each entry is `(column_name, ascending)`. Sorting is performed
    /// per-partition (no inter-partition merge sort).
    pub fn sort_by(&self, columns: &[(impl AsRef<str>, bool)]) -> Result<Self, DatasetError> {
        let col_indices: Vec<usize> = columns
            .iter()
            .map(|(col, _)| {
                self.column_index(col.as_ref())
                    .ok_or_else(|| DatasetError::ColumnNotFound {
                        column: col.as_ref().to_string(),
                        dataset: self.name.clone(),
                    })
            })
            .collect::<Result<_, _>>()?;

        let sort_options: Vec<SortOptions> = columns
            .iter()
            .map(|(_, asc)| SortOptions {
                descending: !asc,
                nulls_first: true,
            })
            .collect();

        let mut sorted_batches = Vec::with_capacity(self.batches.len());
        for batch in &self.batches {
            let sort_cols: Vec<arrow_ord::sort::SortColumn> = col_indices
                .iter()
                .zip(sort_options.iter())
                .map(|(&idx, opts)| arrow_ord::sort::SortColumn {
                    values: batch.column(idx).clone(),
                    options: Some(*opts),
                })
                .collect();

            let indices = lexsort_to_indices(&sort_cols, None)?;
            sorted_batches.push(take_record_batch(batch, &indices)?);
        }

        let col_names: Vec<String> = columns
            .iter()
            .map(|(c, _)| c.as_ref().to_string())
            .collect();
        Ok(AetherDataset::with_schema_and_provenance(
            self.name.clone(),
            self.schema.clone(),
            sorted_batches,
            Provenance::Transform {
                op: format!("sort_by({})", col_names.join(", ")),
                parents: vec![self.name.clone()],
            },
        ))
    }

    /// Boolean-filter rows across all partitions.
    ///
    /// `predicate` is a closure that receives `(row_index_within_batch, batch)`
    /// and returns `true` to keep the row.
    pub fn filter_by<F>(&self, predicate: F) -> Self
    where
        F: Fn(usize, &RecordBatch) -> bool,
    {
        let mut new_batches = Vec::with_capacity(self.batches.len());

        for batch in &self.batches {
            let mut filter_builder = BooleanBufferBuilder::new(batch.num_rows());
            for row in 0..batch.num_rows() {
                filter_builder.append(predicate(row, batch));
            }
            let filter = BooleanArray::new(filter_builder.finish(), None);
            if let Ok(filtered) = filter_record_batch(batch, &filter) {
                if filtered.num_rows() > 0 {
                    new_batches.push(filtered);
                }
            }
        }

        AetherDataset::with_schema_and_provenance(
            self.name.clone(),
            self.schema.clone(),
            new_batches,
            Provenance::Transform {
                op: "filter".into(),
                parents: vec![self.name.clone()],
            },
        )
    }

    /// Apply a scalar function to every element of a numeric column.
    ///
    /// Produces a new dataset where the source column is transformed in-place
    /// (or a new column is appended when `output != column`).
    ///
    /// Handles all numeric Arrow types (`Int*`, `UInt*`, `Float16/32/64`) by
    /// casting to `Float64` before applying `f`. Nulls are preserved.
    ///
    /// # Errors
    ///
    /// - `ColumnNotFound` if `column` does not exist
    /// - `NotNumeric` if the column is not a numeric type
    /// - `Build` if `output` conflicts with an existing column of a different
    ///   type
    ///
    /// # Example
    ///
    /// ```ignore
    /// let transformed = ds.map_column("p_value", "neg_log_p", |x| -x.log10())?;
    /// ```
    pub fn map_column<F>(
        &self,
        column: &str,
        output: &str,
        f: F,
    ) -> Result<Self, DatasetError>
    where
        F: Fn(f64) -> f64,
    {
        let idx = self
            .column_index(column)
            .ok_or_else(|| DatasetError::ColumnNotFound {
                column: column.to_owned(),
                dataset: self.name.clone(),
            })?;

        let field = self.schema.field(idx);
        if !is_numeric_type(field.data_type()) {
            return Err(DatasetError::NotNumeric {
                column: column.to_owned(),
                actual: field.data_type().to_string(),
            });
        }

        // Pre-cast non-Float64 columns once, then map.
        let source_arrays: Vec<Arc<dyn Array>> = self
            .batches
            .iter()
            .map(|b| cast_column_to_f64(b.column(idx)))
            .collect::<Result<_, _>>()
            .map_err(|e| DatasetError::Arrow(e))?;

        let new_batches: Vec<RecordBatch> = self
            .batches
            .iter()
            .zip(source_arrays.iter())
            .map(|(batch, f64_arr)| {
                let src = f64_arr.as_any().downcast_ref::<Float64Array>().unwrap();
                let mut builder = Float64Builder::with_capacity(src.len());

                for i in 0..src.len() {
                    if src.is_null(i) {
                        builder.append_null();
                    } else {
                        builder.append_value(f(src.value(i)));
                    }
                }

                let new_col: Arc<dyn Array> = Arc::new(builder.finish());
                let new_schema = build_map_schema(&self.schema, idx, output);
                let columns = build_map_columns(batch.columns(), idx, new_col, output == column);

                RecordBatch::try_new(Arc::new(new_schema), columns)
                    .map_err(|e| arrow::error::ArrowError::SchemaError(e.to_string()))
            })
            .collect::<Result<_, _>>()
            .map_err(|e| DatasetError::Arrow(e))?;

        Ok(AetherDataset::with_schema_and_provenance(
            self.name.clone(),
            Arc::new(build_map_schema(&self.schema, idx, output)),
            new_batches,
            Provenance::Transform {
                op: format!("map({column} → {output})"),
                parents: vec![self.name.clone()],
            },
        ))
    }

    // -----------------------------------------------------------------------
    // Actions (materialize results)
    // -----------------------------------------------------------------------

    /// Collect all partitions into a single `RecordBatch`.
    ///
    /// Equivalent to Spark's `rdd.collect()`.
    pub fn collect(&self) -> Result<RecordBatch, DatasetError> {
        if self.batches.is_empty() {
            return Ok(RecordBatch::new_empty(self.schema.clone()));
        }
        if self.batches.len() == 1 {
            return Ok(self.batches[0].clone());
        }

        let batches = arrow::compute::concat_batches(&self.schema, &self.batches)?;
        Ok(batches)
    }

    /// Take the first `n` rows as a new dataset (action shorthand).
    ///
    /// Equivalent to Spark's `rdd.take(n)`.
    pub fn take(&self, n: usize) -> Self {
        self.limit(n)
    }

    /// Pretty-print the dataset in Arrow tabular format.
    pub fn pretty_format(&self) -> Result<String, DatasetError> {
        let pretty = arrow::util::pretty::pretty_format_batches(&self.batches)?;
        Ok(pretty.to_string())
    }

    /// Pretty-print the first `n` rows.
    pub fn pretty_head(&self, n: usize) -> Result<String, DatasetError> {
        self.limit(n).pretty_format()
    }

    // -----------------------------------------------------------------------
    // Internal constructors
    // -----------------------------------------------------------------------

    fn with_schema_and_provenance(
        name: String,
        schema: SchemaRef,
        batches: Vec<RecordBatch>,
        provenance: Provenance,
    ) -> Self {
        Self {
            name,
            schema,
            batches,
            provenance,
        }
    }

    /// Set the provenance for this dataset (typically called after
    /// construction via [`AetherDataset::new`]).
    pub fn with_provenance(mut self, prov: Provenance) -> Self {
        self.provenance = prov;
        self
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    /// Extract Float64 column values with null policy handling.
    fn extract_float64_column(
        &self,
        idx: usize,
        null_policy: NullPolicy,
    ) -> Result<Vec<f64>, DatasetError> {
        let column_name = self.schema.field(idx).name().clone();
        let mut result = Vec::with_capacity(self.row_count());
        for batch in &self.batches {
            let arr = batch
                .column(idx)
                .as_any()
                .downcast_ref::<Float64Array>()
                .unwrap();
            collect_f64(arr, &mut result, &column_name, null_policy)?;
        }
        Ok(result)
    }
}

impl fmt::Debug for AetherDataset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AetherDataset")
            .field("name", &self.name)
            .field("rows", &self.row_count())
            .field("columns", &self.column_count())
            .field("partitions", &self.num_partitions())
            .field("provenance", &self.provenance)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Free helpers
// ---------------------------------------------------------------------------

/// Collect f64 values from a Float64Array respecting the null policy.
fn collect_f64(
    arr: &Float64Array,
    out: &mut Vec<f64>,
    column: &str,
    policy: NullPolicy,
) -> Result<(), DatasetError> {
    for v in arr.iter() {
        match (v, policy) {
            (Some(val), _) => out.push(val),
            (None, NullPolicy::DropNulls) => {}
            (None, NullPolicy::Fill(f)) => out.push(f),
            (None, NullPolicy::Reject) => {
                return Err(DatasetError::HasNulls {
                    column: column.to_owned(),
                });
            }
        }
    }
    Ok(())
}

/// Check whether an Arrow `DataType` is a numeric type we can convert to f64.
fn is_numeric_type(dt: &DataType) -> bool {
    matches!(
        dt,
        DataType::Int8
            | DataType::Int16
            | DataType::Int32
            | DataType::Int64
            | DataType::UInt8
            | DataType::UInt16
            | DataType::UInt32
            | DataType::UInt64
            | DataType::Float16
            | DataType::Float32
            | DataType::Float64
    )
}

/// Cast a single numeric column array to Float64.
fn cast_column_to_f64(
    arr: &Arc<dyn Array>,
) -> Result<Arc<dyn Array>, arrow::error::ArrowError> {
    if arr.data_type() == &DataType::Float64 {
        return Ok(arr.clone());
    }
    arrow::compute::cast(arr, &DataType::Float64)
}

/// Build the new schema for a `map_column` result.
///
/// When `output == source_field.name`, the field is replaced in-place.
/// Otherwise the new field is appended.
fn build_map_schema(schema: &SchemaRef, src_idx: usize, output: &str) -> Schema {
    let src_field = schema.field(src_idx);
    let new_field = Field::new(output, DataType::Float64, src_field.is_nullable());

    let mut fields: Vec<Field> = schema.fields().iter().map(|f| (**f).clone()).collect();
    if output == src_field.name() {
        fields[src_idx] = new_field;
    } else {
        // Guard against duplicate column name with wrong type.
        if let Some(existing) = schema.field_with_name(output).ok() {
            if existing.data_type() != &DataType::Float64 {
                // Caller should handle this; return the schema anyway and let
                // RecordBatch::try_new catch the mismatch.
            }
            let idx = schema.index_of(output).unwrap();
            fields[idx] = new_field;
        } else {
            fields.push(new_field);
        }
    }
    Schema::new(fields)
}

/// Build the new column vector for a `map_column` result.
fn build_map_columns(
    columns: &[Arc<dyn Array>],
    src_idx: usize,
    new_col: Arc<dyn Array>,
    replacing: bool,
) -> Vec<Arc<dyn Array>> {
    let mut out: Vec<Arc<dyn Array>> = columns.to_vec();
    if replacing {
        out[src_idx] = new_col;
    } else {
        out.push(new_col);
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use arrow_array::builder::{Float32Builder, Float64Builder, Int64Builder, StringBuilder};

    /// Helper: build a simple 3-column test dataset.
    fn make_test_dataset() -> AetherDataset {
        let mut snp = StringBuilder::new();
        let mut beta = Float64Builder::new();
        let mut pval = Float64Builder::new();

        for (s, b, p) in [
            ("rs1", 0.5, 1e-9),
            ("rs2", -0.3, 0.05),
            ("rs3", 1.2, 3e-8),
            ("rs4", 0.0, 0.5),
        ] {
            snp.append_value(s);
            beta.append_value(b);
            pval.append_value(p);
        }

        let schema = Schema::new(vec![
            Field::new("snp", DataType::Utf8, false),
            Field::new("beta", DataType::Float64, false),
            Field::new("p_value", DataType::Float64, false),
        ]);

        let batch = RecordBatch::try_new(
            Arc::new(schema),
            vec![
                Arc::new(snp.finish()),
                Arc::new(beta.finish()),
                Arc::new(pval.finish()),
            ],
        )
        .unwrap();

        AetherDataset::new("gwas", vec![batch]).unwrap()
    }

    #[test]
    fn test_new_and_metadata() {
        let ds = make_test_dataset();
        assert_eq!(ds.name(), "gwas");
        assert_eq!(ds.row_count(), 4);
        assert_eq!(ds.column_count(), 3);
        assert_eq!(ds.num_partitions(), 1);
        assert!(!ds.is_empty());
        assert!(ds.has_column("beta"));
        assert!(!ds.has_column("missing"));
    }

    #[test]
    fn test_empty_dataset() {
        let schema = Arc::new(Schema::new(vec![Field::new("x", DataType::Float64, true)]));
        let ds = AetherDataset::empty("empty", schema);
        assert!(ds.is_empty());
        assert_eq!(ds.row_count(), 0);
        assert_eq!(ds.column_count(), 1);
    }

    #[test]
    fn test_extract_f64_zero_copy() {
        let ds = make_test_dataset();
        let beta = ds.extract_f64("beta", NullPolicy::default()).unwrap();
        assert_eq!(beta, vec![0.5, -0.3, 1.2, 0.0]);
    }

    #[test]
    fn test_extract_f64_column_not_found() {
        let ds = make_test_dataset();
        let err = ds.extract_f64("nonexistent", NullPolicy::default());
        assert!(matches!(err, Err(DatasetError::ColumnNotFound { .. })));
    }

    #[test]
    fn test_extract_f64_not_numeric() {
        let ds = make_test_dataset();
        let err = ds.extract_f64("snp", NullPolicy::default());
        assert!(matches!(err, Err(DatasetError::NotNumeric { .. })));
    }

    #[test]
    fn test_extract_string() {
        let ds = make_test_dataset();
        let snps = ds.extract_string("snp").unwrap();
        assert_eq!(
            snps,
            vec![
                Some("rs1".into()),
                Some("rs2".into()),
                Some("rs3".into()),
                Some("rs4".into()),
            ]
        );
    }

    #[test]
    fn test_extract_f64_columns() {
        let ds = make_test_dataset();
        let cols = ds
            .extract_f64_columns(&["beta", "p_value"], NullPolicy::default())
            .unwrap();
        assert_eq!(cols.len(), 2);
        assert_eq!(cols[0], vec![0.5, -0.3, 1.2, 0.0]);
        assert_eq!(cols[1], vec![1e-9, 0.05, 3e-8, 0.5]);
    }

    #[test]
    fn test_null_policy_drop() {
        let mut beta = Float64Builder::new();
        beta.append_value(1.0);
        beta.append_null();
        beta.append_value(3.0);

        let schema = Arc::new(Schema::new(vec![Field::new("x", DataType::Float64, true)]));
        let batch = RecordBatch::try_new(schema.clone(), vec![Arc::new(beta.finish())]).unwrap();

        let ds = AetherDataset::with_schema("null_test", schema, vec![batch]);
        let vals = ds.extract_f64("x", NullPolicy::DropNulls).unwrap();
        assert_eq!(vals, vec![1.0, 3.0]);
    }

    #[test]
    fn test_null_policy_fill() {
        let mut beta = Float64Builder::new();
        beta.append_value(1.0);
        beta.append_null();
        beta.append_value(3.0);

        let schema = Arc::new(Schema::new(vec![Field::new("x", DataType::Float64, true)]));
        let batch = RecordBatch::try_new(schema.clone(), vec![Arc::new(beta.finish())]).unwrap();

        let ds = AetherDataset::with_schema("null_test", schema, vec![batch]);
        let vals = ds.extract_f64("x", NullPolicy::Fill(-999.0)).unwrap();
        assert_eq!(vals, vec![1.0, -999.0, 3.0]);
    }

    #[test]
    fn test_null_policy_reject() {
        let mut beta = Float64Builder::new();
        beta.append_value(1.0);
        beta.append_null();
        beta.append_value(3.0);

        let schema = Arc::new(Schema::new(vec![Field::new("x", DataType::Float64, true)]));
        let batch = RecordBatch::try_new(schema.clone(), vec![Arc::new(beta.finish())]).unwrap();

        let ds = AetherDataset::with_schema("null_test", schema, vec![batch]);
        let err = ds.extract_f64("x", NullPolicy::Reject);
        assert!(matches!(err, Err(DatasetError::HasNulls { .. })));
    }

    #[test]
    fn test_select() {
        let ds = make_test_dataset();
        let projected = ds.select(&["snp", "p_value"]).unwrap();
        assert_eq!(projected.column_count(), 2);
        assert!(projected.has_column("snp"));
        assert!(projected.has_column("p_value"));
        assert!(!projected.has_column("beta"));
        assert_eq!(projected.row_count(), 4);
    }

    #[test]
    fn test_drop_columns() {
        let ds = make_test_dataset();
        let dropped = ds.drop_columns(&["beta"]).unwrap();
        assert_eq!(dropped.column_count(), 2);
        assert!(!dropped.has_column("beta"));
    }

    #[test]
    fn test_rename_column() {
        let ds = make_test_dataset();
        let renamed = ds.rename_column("snp", "variant").unwrap();
        assert!(renamed.has_column("variant"));
        assert!(!renamed.has_column("snp"));
    }

    #[test]
    fn test_limit() {
        let ds = make_test_dataset();
        let limited = ds.limit(2);
        assert_eq!(limited.row_count(), 2);
    }

    #[test]
    fn test_filter_by() {
        let ds = make_test_dataset();
        let filtered = ds.filter_by(|row_idx, batch| {
            let pval = batch
                .column(2)
                .as_any()
                .downcast_ref::<Float64Array>()
                .unwrap();
            pval.value(row_idx) < 0.01
        });
        assert_eq!(filtered.row_count(), 2); // rs1 (1e-9) and rs3 (3e-8)
    }

    #[test]
    fn test_union() {
        let ds1 = make_test_dataset();
        let ds2 = ds1.clone();
        let merged = ds1.union(&ds2).unwrap();
        assert_eq!(merged.row_count(), 8);
    }

    #[test]
    fn test_collect() {
        let ds = make_test_dataset();
        let batch = ds.collect().unwrap();
        assert_eq!(batch.num_rows(), 4);
    }

    #[test]
    fn test_collect_empty() {
        let schema = Arc::new(Schema::new(vec![Field::new("x", DataType::Float64, true)]));
        let ds = AetherDataset::empty("empty", schema);
        let batch = ds.collect().unwrap();
        assert_eq!(batch.num_rows(), 0);
    }

    #[test]
    fn test_schema_json() {
        let ds = make_test_dataset();
        let json = ds.schema_json();
        assert_eq!(json["name"], "gwas");
        assert_eq!(json["row_count"], 4);
        assert_eq!(json["column_count"], 3);
    }

    #[test]
    fn test_provenance_chain() {
        let ds = make_test_dataset();
        assert!(matches!(ds.provenance(), Provenance::Manual));

        let selected = ds.select(&["beta"]).unwrap();
        assert!(matches!(
            selected.provenance(),
            Provenance::Transform { op, .. } if op == "select"
        ));

        let limited = selected.limit(1);
        assert!(matches!(
            limited.provenance(),
            Provenance::Transform { op, .. } if op == "limit(1)"
        ));
    }

    #[test]
    fn test_extract_f32() {
        let mut f32_col = Float32Builder::new();
        f32_col.append_value(1.0);
        f32_col.append_value(2.0);

        let schema = Arc::new(Schema::new(vec![Field::new("x", DataType::Float32, false)]));
        let batch = RecordBatch::try_new(schema.clone(), vec![Arc::new(f32_col.finish())]).unwrap();
        let ds = AetherDataset::with_schema("f32_test", schema, vec![batch]);

        let vals = ds.extract_f32("x").unwrap();
        assert_eq!(vals, vec![1.0_f32, 2.0_f32]);
    }

    #[test]
    fn test_extract_f64_from_int64() {
        let mut int_col = Int64Builder::new();
        int_col.append_value(10);
        int_col.append_value(20);

        let schema = Arc::new(Schema::new(vec![Field::new("x", DataType::Int64, false)]));
        let batch = RecordBatch::try_new(schema.clone(), vec![Arc::new(int_col.finish())]).unwrap();
        let ds = AetherDataset::with_schema("int_test", schema, vec![batch]);

        let vals = ds.extract_f64("x", NullPolicy::default()).unwrap();
        assert_eq!(vals, vec![10.0, 20.0]);
    }

    #[test]
    fn test_multi_partition_dataset() {
        let mut snp1 = StringBuilder::new();
        let mut beta1 = Float64Builder::new();
        snp1.append_value("rs1");
        beta1.append_value(0.5);

        let mut snp2 = StringBuilder::new();
        let mut beta2 = Float64Builder::new();
        snp2.append_value("rs2");
        beta2.append_value(-0.3);

        let schema = Arc::new(Schema::new(vec![
            Field::new("snp", DataType::Utf8, false),
            Field::new("beta", DataType::Float64, false),
        ]));

        let b1 = RecordBatch::try_new(
            schema.clone(),
            vec![Arc::new(snp1.finish()), Arc::new(beta1.finish())],
        )
        .unwrap();
        let b2 = RecordBatch::try_new(
            schema.clone(),
            vec![Arc::new(snp2.finish()), Arc::new(beta2.finish())],
        )
        .unwrap();

        let ds = AetherDataset::with_schema("multi", schema, vec![b1, b2]);
        assert_eq!(ds.num_partitions(), 2);
        assert_eq!(ds.row_count(), 2);

        let beta = ds.extract_f64("beta", NullPolicy::default()).unwrap();
        assert_eq!(beta, vec![0.5, -0.3]);

        let collected = ds.collect().unwrap();
        assert_eq!(collected.num_rows(), 2);
    }

    #[test]
    fn test_pretty_format() {
        let ds = make_test_dataset();
        let s = ds.pretty_format().unwrap();
        assert!(s.contains("snp"));
        assert!(s.contains("beta"));
    }

    // ---- map_column tests ----

    #[test]
    fn test_map_column_replace() {
        let ds = make_test_dataset();
        let mapped = ds.map_column("p_value", "p_value", |x| -x.log10()).unwrap();

        assert_eq!(mapped.row_count(), 4);
        assert_eq!(mapped.column_count(), 3); // same columns, not added
        assert!(mapped.has_column("p_value"));

        let vals = mapped.extract_f64("p_value", NullPolicy::DropNulls).unwrap();
        assert_eq!(vals[0], 9.0);  // -log10(1e-9)
        let expected = vec![9.0, -0.05f64.log10(), -(3e-8f64).log10(), -(0.5f64).log10()];
        for (got, want) in vals.iter().zip(expected.iter()) {
            assert!((got - want).abs() < 1e-12, "{got} != {want}");
        }
    }

    #[test]
    fn test_map_column_add_new() {
        let ds = make_test_dataset();
        let mapped = ds.map_column("p_value", "neg_log_p", |x| -x.log10()).unwrap();

        assert_eq!(mapped.column_count(), 4); // snp, beta, p_value, neg_log_p
        assert!(mapped.has_column("p_value"));
        assert!(mapped.has_column("neg_log_p"));

        // Original column untouched
        let orig = mapped.extract_f64("p_value", NullPolicy::DropNulls).unwrap();
        assert_eq!(orig[0], 1e-9);

        // New column has transformed values
        let new = mapped.extract_f64("neg_log_p", NullPolicy::DropNulls).unwrap();
        assert_eq!(new[0], 9.0);
    }

    #[test]
    fn test_map_column_preserves_nulls() {
        let mut beta = Float64Builder::new();
        beta.append_value(1.0);
        beta.append_null();
        beta.append_value(3.0);

        let schema = Schema::new(vec![Field::new("beta", DataType::Float64, true)]);
        let batch = RecordBatch::try_new(
            Arc::new(schema),
            vec![Arc::new(beta.finish())],
        )
        .unwrap();
        let ds = AetherDataset::new("test", vec![batch]).unwrap();

        let mapped = ds.map_column("beta", "beta", |x| x * 2.0).unwrap();
        let vals = mapped.extract_f64("beta", NullPolicy::DropNulls).unwrap();
        assert_eq!(vals.len(), 2);
        assert_eq!(vals[0], 2.0);
        assert_eq!(vals[1], 6.0);
    }

    #[test]
    fn test_map_column_from_int32() {
        let mut b = arrow_array::builder::Int32Builder::new();
        b.append_value(10);
        b.append_value(20);
        b.append_value(30);

        let schema = Schema::new(vec![Field::new("x", DataType::Int32, false)]);
        let batch = RecordBatch::try_new(
            Arc::new(schema),
            vec![Arc::new(b.finish())],
        )
        .unwrap();
        let ds = AetherDataset::new("ints", vec![batch]).unwrap();

        let mapped = ds.map_column("x", "x", |v| v as f64 / 10.0).unwrap();
        let vals = mapped.extract_f64("x", NullPolicy::DropNulls).unwrap();
        assert_eq!(vals, vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn test_map_column_not_found() {
        let ds = make_test_dataset();
        let result = ds.map_column("nonexistent", "out", |x| x);
        assert!(result.is_err());
    }

    #[test]
    fn test_map_column_not_numeric() {
        let ds = make_test_dataset();
        let result = ds.map_column("snp", "out", |x| x);
        assert!(result.is_err());
    }

    #[test]
    fn test_map_column_provenance() {
        let ds = make_test_dataset();
        let mapped = ds.map_column("beta", "beta", |x| x * 2.0).unwrap();
        match mapped.provenance() {
            Provenance::Transform { op, parents } => {
                assert!(op.contains("map(beta"));
                assert_eq!(*parents, vec![String::from("gwas")]);
            }
            other => panic!("expected Transform provenance, got {other:?}"),
        }
    }

    #[test]
    fn test_map_column_multi_partition() {
        let schema = Arc::new(Schema::new(vec![
            Field::new("val", DataType::Float64, false),
        ]));

        let b1 = RecordBatch::try_new(
            schema.clone(),
            vec![Arc::new(Float64Array::from(vec![1.0, 2.0]))],
        )
        .unwrap();
        let b2 = RecordBatch::try_new(
            schema.clone(),
            vec![Arc::new(Float64Array::from(vec![3.0, 4.0]))],
        )
        .unwrap();

        let ds = AetherDataset::with_schema("mp", schema, vec![b1, b2]);
        let mapped = ds.map_column("val", "val", |x| x * x).unwrap();

        assert_eq!(mapped.row_count(), 4);
        assert_eq!(mapped.num_partitions(), 2);
        let vals = mapped.extract_f64("val", NullPolicy::DropNulls).unwrap();
        assert_eq!(vals, vec![1.0, 4.0, 9.0, 16.0]);
    }
}
