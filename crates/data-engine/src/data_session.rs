//! DataFusion-based session for Iceberg tables.
//!
//! [`DataSession`] wraps a `SessionContext` with the Iceberg catalog
//! mounted, providing table discovery (`list_tables`), schema
//! inspection (`describe_table`), and table materialisation
//! (`read_table` / `peek_table`) through the DataFusion catalog API.

use std::sync::Arc;

use anyhow::Result;
use arrow_array::RecordBatch;
use datafusion::prelude::SessionContext;
use serde_json::{Value, json};

use crate::Dataset;
use crate::DatasetStore;
use crate::Provenance;
use crate::datalake::Datalake;
use crate::error::DatasetError;

pub struct DataSession {
    datalake: Arc<Datalake>,
    ctx: SessionContext,
}

impl DataSession {
    /// Create a new workspace with the Iceberg catalog registered as `"iceberg"`.
    pub async fn new() -> Result<Self> {
        let datalake = Arc::new(Datalake::default());
        let ctx = datalake.get_ctx().await?;
        Ok(Self { datalake, ctx })
    }

    /// Return a reference to the underlying `SessionContext`.
    pub fn ctx(&self) -> &SessionContext {
        &self.ctx
    }

    /// Return the shared Iceberg REST catalog, initializing it on first use.
    ///
    /// Namespace/table CRUD tools route through here so the catalog's
    /// internal `OnceCell` cache is shared across all tool invocations
    /// within this workspace.
    pub async fn catalog(&self) -> Result<Arc<iceberg_catalog_rest::RestCatalog>> {
        self.datalake.get_catalog().await
    }

    /// List all tables across all namespaces in the `"iceberg"` catalog.
    ///
    /// Uses DataFusion's catalog API (not the Iceberg REST API).
    pub fn list_tables(&self) -> Result<Vec<(String, String)>> {
        let catalog = self
            .ctx
            .catalog("iceberg")
            .ok_or_else(|| anyhow::anyhow!("catalog 'iceberg' not registered"))?;

        let mut result = Vec::new();
        for schema_name in catalog.schema_names() {
            if let Some(schema) = catalog.schema(&schema_name) {
                for table_name in schema.table_names() {
                    result.push((schema_name.clone(), table_name));
                }
            }
        }
        Ok(result)
    }

    /// List tables in a specific namespace (schema).
    pub fn list_tables_in_namespace(&self, namespace: &str) -> Result<Vec<String>> {
        let catalog = self
            .ctx
            .catalog("iceberg")
            .ok_or_else(|| anyhow::anyhow!("catalog 'iceberg' not registered"))?;

        let schema = catalog
            .schema(namespace)
            .ok_or_else(|| anyhow::anyhow!("schema '{namespace}' not found"))?;

        Ok(schema.table_names())
    }

    /// Describe a table: return its Arrow schema as JSON.
    ///
    /// Returns `{ "table": "...", "columns": [...], "column_count": N }`
    /// where each column has `"name"`, `"type"`, `"nullable"`.
    pub async fn describe_table(&self, namespace: &str, table: &str) -> Result<Value> {
        let catalog = self
            .ctx
            .catalog("iceberg")
            .ok_or_else(|| anyhow::anyhow!("catalog 'iceberg' not registered"))?;

        let schema = catalog
            .schema(namespace)
            .ok_or_else(|| anyhow::anyhow!("schema '{namespace}' not found"))?;

        let provider = schema
            .table(table)
            .await?
            .ok_or_else(|| anyhow::anyhow!("table '{namespace}.{table}' not found"))?;

        let arrow_schema = provider.schema();
        let columns: Vec<Value> = arrow_schema
            .fields()
            .iter()
            .map(|f| {
                json!({
                    "name": f.name(),
                    "type": f.data_type().to_string(),
                    "nullable": f.is_nullable(),
                })
            })
            .collect();

        Ok(json!({
            "table": format!("{namespace}.{table}"),
            "columns": columns,
            "column_count": columns.len(),
        }))
    }

    /// Execute a read-only SQL query and return the result batches plus a pretty-formatted string.
    pub async fn sql_query(
        &self,
        sql: &str,
    ) -> Result<(Vec<datafusion::arrow::array::RecordBatch>, String)> {
        let batches = self.ctx.sql(sql).await?.collect().await?;
        let pretty = arrow::util::pretty::pretty_format_batches(&batches)?.to_string();
        Ok((batches, pretty))
    }

    /// Load an Iceberg table into a named in-memory [`Dataset`], registering it
    /// in `store` under `name` (overwriting any existing entry).
    ///
    /// This is the L1 ingestion primitive: data flows from a persistent
    /// Iceberg table (registered in the `iceberg` catalog) into an
    /// in-memory dataset for analysis. Optionally project `columns`, apply a
    /// `filter` SQL expression, and cap with `limit`.
    ///
    /// `namespace` must be a single top-level segment (DataFusion catalog
    /// limitation), e.g. `"analytics"`.
    pub async fn read_table(
        &self,
        store: &DatasetStore,
        name: &str,
        namespace: &str,
        table: &str,
        columns: Option<&[String]>,
        filter: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Arc<Dataset>, DatasetError> {
        let sql = build_table_sql(namespace, table, columns, filter, limit);
        let plan = self.ctx.sql(&sql).await?;
        let table_ident = format!("{namespace}.{table}");

        let schema: Arc<arrow_schema::Schema> = Arc::new(plan.schema().as_arrow().clone());
        let batches: Vec<RecordBatch> = plan
            .collect()
            .await?
            .into_iter()
            .filter(|b| b.num_rows() > 0)
            .collect();
        let ds = Dataset::with_schema(name, schema, batches)
            .with_provenance(Provenance::Table { table: table_ident });

        Ok(store.put_overwrite(ds).await)
    }

    /// Peek at an Iceberg table **without materializing** it into the store.
    ///
    /// Returns a transient [`Dataset`] that is never registered. Use it for
    /// read-only preview/describe operations where the agent just wants to
    /// look at rows without occupying working memory or polluting the
    /// namespace of named datasets.
    pub async fn peek_table(
        &self,
        namespace: &str,
        table: &str,
        columns: Option<&[String]>,
        filter: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Dataset, DatasetError> {
        let sql = build_table_sql(namespace, table, columns, filter, limit);
        let plan = self.ctx.sql(&sql).await?;
        let table_ident = format!("{namespace}.{table}");

        let schema: Arc<arrow_schema::Schema> = Arc::new(plan.schema().as_arrow().clone());
        let batches: Vec<RecordBatch> = plan
            .collect()
            .await?
            .into_iter()
            .filter(|b| b.num_rows() > 0)
            .collect();

        Ok(Dataset::with_schema("peek", schema, batches)
            .with_provenance(Provenance::Table { table: table_ident }))
    }
}

/// Build a `SELECT … FROM iceberg."ns"."table" [WHERE …] [LIMIT …]` query.
///
/// Must use the dotted 3-segment form `catalog.schema.table`; the comma
/// form is parsed as a SQL cross-join and resolves under the wrong catalog.
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

/// Quote a SQL identifier with double quotes, escaping embedded quotes.
fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let sql = build_table_sql("analytics", "gwas", None, Some("p_value < 5e-8"), Some(100));
        assert_eq!(
            sql,
            "SELECT * FROM iceberg.\"analytics\".\"gwas\" WHERE p_value < 5e-8 LIMIT 100"
        );

        // Empty filter string is ignored.
        let sql = build_table_sql("analytics", "gwas", None, Some("   "), None);
        assert_eq!(sql, "SELECT * FROM iceberg.\"analytics\".\"gwas\"");

        // Empty column list falls back to *.
        let sql = build_table_sql("analytics", "gwas", Some(&[]), None, None);
        assert_eq!(sql, "SELECT * FROM iceberg.\"analytics\".\"gwas\"");
    }
}
