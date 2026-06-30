//! Named registry of in-memory datasets with SQL execution capability.

use std::collections::HashMap;
use std::sync::Arc;

use arrow_array::RecordBatch;
use datafusion::datasource::MemTable;
use datafusion::prelude::SessionContext;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use super::Dataset;
use crate::error::DatasetError;

/// Lightweight metadata for listing datasets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetInfo {
    pub id: String,
    pub row_count: usize,
    pub column_count: usize,
    pub columns: Vec<ColumnInfo>,
}

/// Per-column metadata for [`DatasetInfo`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
}

/// Named registry of in-memory datasets, owned by the agent.
///
/// Internally holds a private `SessionContext` used only to execute
/// DataFusion SQL over the registered in-memory datasets (see
/// [`Self::map_expr`] and [`Self::sql_query`]). It does **not** carry any
/// external catalog (e.g. Iceberg) — interaction with persistent storage is
/// the responsibility of [`crate::data_session::DataSession`].
pub struct DatasetStore {
    // TODO: Change to Dashmap to avoid heavy lock during data writing.
    datasets: RwLock<HashMap<String, Arc<Dataset>>>,
    ctx: SessionContext,
}

impl DatasetStore {
    /// Create a new empty store. The internal `SessionContext` is created
    /// internally and only ever holds in-memory `MemTable`s.
    pub fn new() -> Self {
        Self {
            datasets: RwLock::new(HashMap::new()),
            ctx: SessionContext::new(),
        }
    }

    /// Register a dataset. Errors if the name already exists.
    pub async fn put(&self, dataset: Dataset) -> Result<(), DatasetError> {
        let name = dataset.id().to_owned();
        let mut map = self.datasets.write().await;
        if map.contains_key(&name) {
            return Err(DatasetError::Build {
                message: format!("dataset '{name}' already exists"),
            });
        }
        map.insert(name, Arc::new(dataset));
        Ok(())
    }

    /// Register a dataset, replacing any existing entry with the same name.
    /// Returns the inserted `Arc<Dataset>` for convenience.
    pub async fn put_overwrite(&self, dataset: Dataset) -> Arc<Dataset> {
        let wrapped = Arc::new(dataset);
        let mut map = self.datasets.write().await;
        map.insert(wrapped.id().to_owned(), wrapped.clone());
        wrapped
    }

    /// Retrieve a dataset by name.
    pub async fn get(&self, name: &str) -> Result<Arc<Dataset>, DatasetError> {
        let map = self.datasets.read().await;
        map.get(name)
            .cloned()
            .ok_or_else(|| DatasetError::NotFound {
                name: name.to_owned(),
            })
    }

    /// Remove a dataset from the store.
    pub async fn drop(&self, name: &str) -> Result<(), DatasetError> {
        let mut map = self.datasets.write().await;
        map.remove(name)
            .map(|_| ())
            .ok_or_else(|| DatasetError::NotFound {
                name: name.to_owned(),
            })
    }

    /// List all registered datasets with their metadata.
    pub async fn list(&self) -> Vec<DatasetInfo> {
        let map = self.datasets.read().await;
        let mut infos: Vec<DatasetInfo> = map
            .values()
            .map(|ds| DatasetInfo {
                id: ds.id().to_owned(),
                row_count: ds.row_count(),
                column_count: ds.column_count(),
                columns: ds
                    .column_names()
                    .map(|n| {
                        let f = ds.field(n).unwrap();
                        ColumnInfo {
                            name: n.to_owned(),
                            data_type: f.data_type().to_string(),
                            nullable: f.is_nullable(),
                        }
                    })
                    .collect(),
            })
            .collect();
        infos.sort_by(|a, b| a.id.cmp(&b.id));
        infos
    }

    /// Check whether a dataset with the given name exists.
    pub async fn exists(&self, name: &str) -> bool {
        self.datasets.read().await.contains_key(name)
    }

    /// Apply a DataFusion SQL expression to a column, producing a transformed
    /// dataset.
    ///
    /// Uses DataFusion's expression parser, so `expr` can be any valid SQL
    /// scalar expression: `"-log10(p_value)"`, `"beta * 2"`, `"abs(z_score)"`,
    /// etc. The expression may reference **any** column in the source dataset.
    ///
    /// When `output_col == column`, the transformed column replaces the original
    /// in-place. Otherwise a new column is appended.
    ///
    /// The result is registered under `name` (overwriting if it already exists).
    pub async fn map_expr(
        &self,
        name: &str,
        source: &str,
        column: &str,
        expr: &str,
        output_col: &str,
    ) -> Result<Arc<Dataset>, DatasetError> {
        self.register_all_as_tables().await?;

        let all_cols: Vec<String> = {
            let ds = self.get(source).await?;
            ds.column_names().map(String::from).collect()
        };

        // Build column list excluding the column being replaced (if in-place).
        let select_cols: Vec<String> = if output_col == column {
            all_cols
                .iter()
                .map(|c| {
                    if c == column {
                        format!("{expr} AS \"{output_col}\"")
                    } else {
                        format!("\"{c}\"")
                    }
                })
                .collect()
        } else {
            let mut cols: Vec<String> = all_cols.iter().map(|c| format!("\"{c}\"")).collect();
            cols.push(format!("{expr} AS \"{output_col}\""));
            cols
        };

        let sql = format!("SELECT {} FROM \"{}\"", select_cols.join(", "), source,);

        let plan = self.ctx.sql(&sql).await?;
        let ds = self
            .collect_df(
                name,
                plan,
                super::Provenance::Transform {
                    op: format!("map({column} → {output_col}, expr={expr})"),
                    parents: vec![source.to_owned()],
                },
            )
            .await?;

        // Overwrite if same name as source, otherwise insert new.
        if name == source {
            let mut datasets = self.datasets.write().await;
            datasets.insert(name.to_owned(), Arc::new(ds));
            Ok(Arc::clone(datasets.get(name).unwrap()))
        } else {
            Ok(self.insert_dataset(ds).await)
        }
    }

    /// Execute a read-only SQL query and return the result as a transient
    /// `Dataset` (not registered in the store).
    pub async fn sql_query(&self, sql: &str) -> Result<Dataset, DatasetError> {
        self.register_all_as_tables().await?;

        let plan = self.ctx.sql(sql).await?;
        self.collect_df("query_result", plan, super::Provenance::Manual)
            .await
    }

    /// Collect a DataFusion `DataFrame` into an `Dataset` with the
    /// given name and provenance.
    ///
    /// Shared by [`map_expr`] and [`sql_query`].
    async fn collect_df(
        &self,
        name: &str,
        df: datafusion::dataframe::DataFrame,
        provenance: super::Provenance,
    ) -> Result<Dataset, DatasetError> {
        let schema: Arc<arrow_schema::Schema> = Arc::new(df.schema().as_arrow().clone());
        let batches: Vec<RecordBatch> = df
            .collect()
            .await?
            .into_iter()
            .filter(|b| b.num_rows() > 0)
            .collect();
        Ok(Dataset::with_schema(name, schema, batches).with_provenance(provenance))
    }

    /// Insert a dataset into the registry, returning a shared `Arc`.
    async fn insert_dataset(&self, dataset: Dataset) -> Arc<Dataset> {
        let wrapped = Arc::new(dataset);
        let mut map = self.datasets.write().await;
        map.insert(wrapped.id().to_owned(), wrapped.clone());
        wrapped
    }

    /// Return a reference to the underlying `SessionContext`.
    pub fn ctx(&self) -> &SessionContext {
        &self.ctx
    }

    /// Register all stored datasets as DataFusion temporary tables.
    async fn register_all_as_tables(&self) -> Result<(), DatasetError> {
        let map = self.datasets.read().await;
        for ds in map.values() {
            self.register_as_table(ds)?;
        }
        Ok(())
    }

    /// Register a single dataset as a DataFusion `MemTable`.
    fn register_as_table(&self, ds: &Dataset) -> Result<(), DatasetError> {
        let partitions: Vec<Vec<_>> = ds
            .batches()
            .iter()
            .filter(|b| b.num_rows() > 0)
            .map(|b| vec![b.clone()])
            .collect();

        // MemTable requires at least one partition. If the dataset has no
        // non-empty batches (e.g. an empty result), register an empty
        // RecordBatch partition so that subsequent SQL can still reference
        // the table schema (SELECT will return 0 rows).
        let partitions = if partitions.is_empty() {
            vec![vec![RecordBatch::new_empty(ds.schema().clone())]]
        } else {
            partitions
        };

        let table = MemTable::try_new(ds.schema().clone(), partitions)?;
        // Ignore error if table doesn't exist yet.
        let _ = self.ctx.deregister_table(ds.id());
        self.ctx.register_table(ds.id(), Arc::new(table))?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NullPolicy;
    use arrow_array::RecordBatch;
    use arrow_array::builder::Float64Builder;
    use arrow_schema::{DataType, Field, Schema};

    fn make_simple_dataset(name: &str) -> Dataset {
        let mut x = Float64Builder::new();
        let mut y = Float64Builder::new();
        for i in 0..5 {
            x.append_value(i as f64);
            y.append_value((i as f64) * 2.0);
        }
        let schema = Arc::new(Schema::new(vec![
            Field::new("x", DataType::Float64, false),
            Field::new("y", DataType::Float64, false),
        ]));
        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![Arc::new(x.finish()), Arc::new(y.finish())],
        )
        .unwrap();
        Dataset::with_schema(name, schema, vec![batch])
    }

    #[tokio::test]
    async fn test_put_get_drop() {
        let store = DatasetStore::new();
        let ds = make_simple_dataset("test_ds");

        assert!(!store.exists("test_ds").await);
        store.put(ds).await.unwrap();
        assert!(store.exists("test_ds").await);

        let fetched = store.get("test_ds").await.unwrap();
        assert_eq!(fetched.row_count(), 5);

        store.drop("test_ds").await.unwrap();
        assert!(!store.exists("test_ds").await);
    }

    #[tokio::test]
    async fn test_put_duplicate_rejected() {
        let store = DatasetStore::new();
        store.put(make_simple_dataset("dup")).await.unwrap();
        let result = store.put(make_simple_dataset("dup")).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_put_overwrite() {
        let store = DatasetStore::new();
        store.put(make_simple_dataset("ow")).await.unwrap();
        store.put_overwrite(make_simple_dataset("ow")).await;
        assert!(store.exists("ow").await);
    }

    #[tokio::test]
    async fn test_list() {
        let store = DatasetStore::new();
        store.put(make_simple_dataset("a")).await.unwrap();
        store.put(make_simple_dataset("b")).await.unwrap();

        let infos = store.list().await;
        assert_eq!(infos.len(), 2);
        // Should be sorted alphabetically
        assert_eq!(infos[0].id, "a");
        assert_eq!(infos[1].id, "b");
        assert_eq!(infos[0].row_count, 5);
        assert_eq!(infos[0].column_count, 2);
    }

    #[tokio::test]
    async fn test_sql_query_on_registered_dataset() {
        let store = DatasetStore::new();
        store.put(make_simple_dataset("nums")).await.unwrap();

        let result = store
            .sql_query("SELECT x, y FROM nums WHERE x > 2")
            .await
            .unwrap();
        assert_eq!(result.row_count(), 2); // x=3, x=4
    }

    #[tokio::test]
    async fn test_map_expr_add_column() {
        let store = DatasetStore::new();
        store.put(make_simple_dataset("nums")).await.unwrap();

        // Add a new column y_squared = y * y
        store
            .map_expr("nums", "nums", "y", "y * y", "y_squared")
            .await
            .unwrap();

        let ds = store.get("nums").await.unwrap();
        assert!(ds.has_column("y_squared"));
        let vals = ds.extract_f64("y_squared", NullPolicy::DropNulls).unwrap();
        // make_simple_dataset has y = [0.0, 2.0, 4.0, 6.0, 8.0]
        assert_eq!(vals, vec![0.0, 4.0, 16.0, 36.0, 64.0]);
    }

    #[tokio::test]
    async fn test_map_expr_replace_column() {
        let store = DatasetStore::new();
        store.put(make_simple_dataset("nums")).await.unwrap();

        // Replace y with y * 10
        store
            .map_expr("nums", "nums", "y", "y * 10", "y")
            .await
            .unwrap();

        let ds = store.get("nums").await.unwrap();
        assert_eq!(ds.column_count(), 2); // still 2 columns, not 3
        let vals = ds.extract_f64("y", NullPolicy::DropNulls).unwrap();
        assert_eq!(vals, vec![0.0, 20.0, 40.0, 60.0, 80.0]);
    }

    #[tokio::test]
    async fn test_map_expr_log() {
        let store = DatasetStore::new();
        store.put(make_simple_dataset("nums")).await.unwrap();

        // Log transform: ln(y) — skip y=0 (ln(0) = -inf)
        store
            .map_expr("nums", "nums", "y", "ln(y)", "log_y")
            .await
            .unwrap();

        let ds = store.get("nums").await.unwrap();
        let vals = ds.extract_f64("log_y", NullPolicy::DropNulls).unwrap();
        // y = [0.0, 2.0, 4.0, 6.0, 8.0] → ln = [-inf, 0.693.., 1.386.., 1.791.., 2.079..]
        assert_eq!(vals.len(), 5);
        assert!(vals[0].is_infinite() && vals[0].is_sign_negative()); // ln(0) = -inf
        for (got, want) in vals[1..].iter().zip([2.0f64, 4.0, 6.0, 8.0].iter()) {
            assert!((got - want.ln()).abs() < 1e-12, "{got} != {}", want.ln());
        }
    }

    #[tokio::test]
    async fn test_get_not_found() {
        let store = DatasetStore::new();
        let err = store.get("nope").await;
        assert!(matches!(err, Err(DatasetError::NotFound { .. })));
    }

    #[tokio::test]
    async fn test_drop_not_found() {
        let store = DatasetStore::new();
        let err = store.drop("nope").await;
        assert!(matches!(err, Err(DatasetError::NotFound { .. })));
    }
}
