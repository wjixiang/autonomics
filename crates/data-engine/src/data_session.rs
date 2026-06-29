//! DataFusion-based session for Iceberg tables.
//!
//! [`DataSession`] wraps a `SessionContext` with the Iceberg catalog
//! mounted, providing table discovery (`list_tables`) and schema
//! inspection (`describe_table`) through the DataFusion catalog API.

use std::sync::Arc;

use anyhow::Result;
use datafusion::prelude::SessionContext;
use serde_json::{Value, json};

use crate::datalake::Datalake;

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
}
