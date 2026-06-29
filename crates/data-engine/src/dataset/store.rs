//! Named registry of in-memory datasets with SQL execution capability.

use std::collections::HashMap;
use std::sync::Arc;

use arrow_array::RecordBatch;
use datafusion::datasource::MemTable;
use datafusion::prelude::SessionContext;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use super::AetherDataset;
use crate::data_session::DataSession;
use crate::error::DatasetError;

/// Lightweight metadata for listing datasets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetInfo {
    pub name: String,
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

/// Named registry of in-memory datasets, backed by a shared DataFusion
/// `SessionContext` for SQL-based transformations.
///
/// This is the agent's **working memory**: datasets created from Iceberg
/// queries or upstream transformations are registered here so they can be
/// referenced by name in subsequent SQL operations.
pub struct DatasetStore {
    datasets: RwLock<HashMap<String, Arc<AetherDataset>>>,
    ctx: SessionContext,
}

impl DatasetStore {
    /// Create a new empty store with a DataFusion session context.
    pub fn new(ctx: SessionContext) -> Self {
        Self {
            datasets: RwLock::new(HashMap::new()),
            ctx,
        }
    }

    /// Create a store reusing the `SessionContext` from an existing
    /// [`DataSession`].
    pub fn from_workspace(workspace: &DataSession) -> Self {
        Self::new(workspace.ctx().clone())
    }

    /// Register a dataset. Errors if the name already exists.
    pub async fn put(&self, dataset: AetherDataset) -> Result<(), DatasetError> {
        let name = dataset.name().to_owned();
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
    pub async fn put_overwrite(&self, dataset: AetherDataset) {
        let name = dataset.name().to_owned();
        let mut map = self.datasets.write().await;
        map.insert(name, Arc::new(dataset));
    }

    /// Retrieve a dataset by name.
    pub async fn get(&self, name: &str) -> Result<Arc<AetherDataset>, DatasetError> {
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
                name: ds.name().to_owned(),
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
        infos.sort_by(|a, b| a.name.cmp(&b.name));
        infos
    }

    /// Check whether a dataset with the given name exists.
    pub async fn exists(&self, name: &str) -> bool {
        self.datasets.read().await.contains_key(name)
    }

    /// Execute a SQL query and register the result as a named dataset.
    ///
    /// The SQL may reference:
    /// - Iceberg tables via the `iceberg` catalog (e.g. `iceberg.ns.table`)
    /// - Other registered datasets (they are automatically registered as
    ///   temporary tables before query execution).
    pub async fn sql_to_dataset(
        &self,
        name: &str,
        sql: &str,
    ) -> Result<Arc<AetherDataset>, DatasetError> {
        // Ensure all stored datasets are available as DataFusion tables.
        self.register_all_as_tables().await?;

        let plan = self.ctx.sql(sql).await?;
        let ds = self.collect_df(name, plan, super::Provenance::Query {
            sql: sql.to_owned(),
        })
        .await?;

        Ok(self.insert_dataset(ds).await)
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
    ) -> Result<Arc<AetherDataset>, DatasetError> {
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
            let mut cols: Vec<String> = all_cols
                .iter()
                .map(|c| format!("\"{c}\""))
                .collect();
            cols.push(format!("{expr} AS \"{output_col}\""));
            cols
        };

        let sql = format!(
            "SELECT {} FROM \"{}\"",
            select_cols.join(", "),
            source,
        );

        let plan = self.ctx.sql(&sql).await?;
        let ds = self.collect_df(name, plan, super::Provenance::Transform {
            op: format!("map({column} → {output_col}, expr={expr})"),
            parents: vec![source.to_owned()],
        })
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

    /// Load an Iceberg table into a named `AetherDataset`.
    ///
    /// This is the primary ingestion primitive: data flows from a persistent
    /// Iceberg table (registered in the `iceberg` catalog) into an in-memory
    /// dataset for analysis. Optionally project `columns`, apply a `filter`
    /// SQL expression, and cap with `limit`.
    ///
    /// `namespace` must be a single top-level segment (DataFusion catalog
    /// limitation), e.g. `"analytics"`.
    pub async fn read_table(
        &self,
        name: &str,
        namespace: &str,
        table: &str,
        columns: Option<&[String]>,
        filter: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Arc<AetherDataset>, DatasetError> {
        let sql = build_table_sql(namespace, table, columns, filter, limit);
        let plan = self.ctx.sql(&sql).await?;
        let table_ident = format!("{namespace}.{table}");
        let ds = self.collect_df(name, plan, super::Provenance::Table {
            table: table_ident,
        })
        .await?;

        Ok(self.insert_dataset(ds).await)
    }

    /// Peek at an Iceberg table **without materializing** it into the store.
    ///
    /// This is the L2 inspection counterpart to [`read_table`]: it runs the
    /// same kind of `SELECT … FROM iceberg.{ns}.{table}` query but returns a
    /// **transient** [`AetherDataset`] that is never registered. Use it for
    /// read-only preview/describe operations where the agent just wants to
    /// look at rows without occupying working memory or polluting the
    /// namespace of named datasets.
    ///
    /// Because it queries the Iceberg catalog directly (like [`read_table`]),
    /// it does not need to register any in-memory datasets first.
    pub async fn peek_table(
        &self,
        namespace: &str,
        table: &str,
        columns: Option<&[String]>,
        filter: Option<&str>,
        limit: Option<usize>,
    ) -> Result<AetherDataset, DatasetError> {
        let sql = build_table_sql(namespace, table, columns, filter, limit);
        let plan = self.ctx.sql(&sql).await?;
        let table_ident = format!("{namespace}.{table}");
        self.collect_df("peek", plan, super::Provenance::Table {
            table: table_ident,
        })
        .await
    }

    /// Execute a read-only SQL query and return the result as a transient
    /// `AetherDataset` (not registered in the store).
    pub async fn sql_query(&self, sql: &str) -> Result<AetherDataset, DatasetError> {
        self.register_all_as_tables().await?;

        let plan = self.ctx.sql(sql).await?;
        self.collect_df("query_result", plan, super::Provenance::Manual)
            .await
    }

    /// Collect a DataFusion `DataFrame` into an `AetherDataset` with the
    /// given name and provenance.
    ///
    /// Shared by [`sql_to_dataset`], [`read_table`], and [`sql_query`].
    async fn collect_df(
        &self,
        name: &str,
        df: datafusion::dataframe::DataFrame,
        provenance: super::Provenance,
    ) -> Result<AetherDataset, DatasetError> {
        let schema: Arc<arrow_schema::Schema> = Arc::new(df.schema().as_arrow().clone());
        let batches: Vec<RecordBatch> = df
            .collect()
            .await?
            .into_iter()
            .filter(|b| b.num_rows() > 0)
            .collect();
        Ok(AetherDataset::with_schema(name, schema, batches).with_provenance(provenance))
    }

    /// Insert a dataset into the registry, returning a shared `Arc`.
    async fn insert_dataset(
        &self,
        dataset: AetherDataset,
    ) -> Arc<AetherDataset> {
        let wrapped = Arc::new(dataset);
        let mut map = self.datasets.write().await;
        map.insert(wrapped.name().to_owned(), wrapped.clone());
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
    fn register_as_table(&self, ds: &AetherDataset) -> Result<(), DatasetError> {
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
        let _ = self.ctx.deregister_table(ds.name());
        self.ctx.register_table(ds.name(), Arc::new(table))?;
        Ok(())
    }
}

/// Quote a SQL identifier with double quotes, escaping embedded quotes.
fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Build a `SELECT … FROM iceberg."ns"."table" [WHERE …] [LIMIT …]` query.
///
/// Shared by [`DatasetStore::read_table`] (materializing) and
/// [`DatasetStore::peek_table`] (transient) so the projection/filter/limit
/// logic stays in one place.
fn build_table_sql(
    namespace: &str,
    table: &str,
    columns: Option<&[String]>,
    filter: Option<&str>,
    limit: Option<usize>,
) -> String {
    let projection = match columns {
        Some(cols) if !cols.is_empty() => cols
            .iter()
            .map(|c| quote_ident(c.trim()))
            .collect::<Vec<_>>()
            .join(", "),
        _ => "*".to_string(),
    };

    let mut sql = format!(
        "SELECT {projection} FROM iceberg.{}.{}",
        quote_ident(namespace),
        quote_ident(table),
    );
    if let Some(clause) = filter.map(str::trim).filter(|s| !s.is_empty()) {
        sql.push_str(" WHERE ");
        sql.push_str(clause);
    }
    if let Some(n) = limit {
        sql.push_str(&format!(" LIMIT {n}"));
    }
    sql
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

    fn make_simple_dataset(name: &str) -> AetherDataset {
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
        AetherDataset::with_schema(name, schema, vec![batch])
    }

    #[tokio::test]
    async fn test_put_get_drop() {
        let ctx = SessionContext::new();
        let store = DatasetStore::new(ctx);
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
        let ctx = SessionContext::new();
        let store = DatasetStore::new(ctx);
        store.put(make_simple_dataset("dup")).await.unwrap();
        let result = store.put(make_simple_dataset("dup")).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_put_overwrite() {
        let ctx = SessionContext::new();
        let store = DatasetStore::new(ctx);
        store.put(make_simple_dataset("ow")).await.unwrap();
        store.put_overwrite(make_simple_dataset("ow")).await;
        assert!(store.exists("ow").await);
    }

    #[tokio::test]
    async fn test_list() {
        let ctx = SessionContext::new();
        let store = DatasetStore::new(ctx);
        store.put(make_simple_dataset("a")).await.unwrap();
        store.put(make_simple_dataset("b")).await.unwrap();

        let infos = store.list().await;
        assert_eq!(infos.len(), 2);
        // Should be sorted alphabetically
        assert_eq!(infos[0].name, "a");
        assert_eq!(infos[1].name, "b");
        assert_eq!(infos[0].row_count, 5);
        assert_eq!(infos[0].column_count, 2);
    }

    #[tokio::test]
    async fn test_sql_query_on_registered_dataset() {
        let ctx = SessionContext::new();
        let store = DatasetStore::new(ctx);
        store.put(make_simple_dataset("nums")).await.unwrap();

        let result = store
            .sql_query("SELECT x, y FROM nums WHERE x > 2")
            .await
            .unwrap();
        assert_eq!(result.row_count(), 2); // x=3, x=4
    }

    #[tokio::test]
    async fn test_sql_to_dataset() {
        let ctx = SessionContext::new();
        let store = DatasetStore::new(ctx);
        store.put(make_simple_dataset("nums")).await.unwrap();

        store
            .sql_to_dataset("filtered", "SELECT * FROM nums WHERE y > 4")
            .await
            .unwrap();

        assert!(store.exists("filtered").await);
        let ds = store.get("filtered").await.unwrap();
        assert_eq!(ds.row_count(), 2); // y=6, y=8
    }

    #[tokio::test]
    async fn test_map_expr_add_column() {
        let ctx = SessionContext::new();
        let store = DatasetStore::new(ctx);
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
        let ctx = SessionContext::new();
        let store = DatasetStore::new(ctx);
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
        let ctx = SessionContext::new();
        let store = DatasetStore::new(ctx);
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
        let ctx = SessionContext::new();
        let store = DatasetStore::new(ctx);
        let err = store.get("nope").await;
        assert!(matches!(err, Err(DatasetError::NotFound { .. })));
    }

    #[tokio::test]
    async fn test_drop_not_found() {
        let ctx = SessionContext::new();
        let store = DatasetStore::new(ctx);
        let err = store.drop("nope").await;
        assert!(matches!(err, Err(DatasetError::NotFound { .. })));
    }

    #[test]
    fn test_build_table_sql_projections() {
        // Dotted 3-part form: catalog.schema.table — the comma form is a SQL
        // cross-join and resolves under the wrong catalog.
        let sql = build_table_sql("analytics", "gwas", None, None, None);
        assert_eq!(sql, "SELECT * FROM iceberg.\"analytics\".\"gwas\"");

        // Column projection + identifier quoting (embedded quote escaped).
        let sql = build_table_sql(
            "analytics",
            "gwas",
            Some(&["snp".into(), "be\"ta".into()]),
            None,
            None,
        );
        assert_eq!(
            sql,
            "SELECT \"snp\", \"be\"\"ta\" FROM iceberg.\"analytics\".\"gwas\""
        );

        // Filter + limit.
        let sql = build_table_sql(
            "analytics",
            "gwas",
            None,
            Some("p_value < 5e-8"),
            Some(100),
        );
        assert_eq!(
            sql,
            "SELECT * FROM iceberg.\"analytics\".\"gwas\" WHERE p_value < 5e-8 LIMIT 100"
        );

        // Empty filter string is ignored.
        let sql = build_table_sql("analytics", "gwas", None, Some("   "), None);
        assert_eq!(sql, "SELECT * FROM iceberg.\"analytics\".\"gwas\"");

        // Empty column list falls back to *.
        let sql = build_table_sql(
            "analytics",
            "gwas",
            Some(&[]),
            None,
            None,
        );
        assert_eq!(sql, "SELECT * FROM iceberg.\"analytics\".\"gwas\"");
    }
}
