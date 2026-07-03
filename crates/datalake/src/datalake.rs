use crate::config;
use crate::config::IcebergConfig;
use datafusion::prelude::SessionContext;
use iceberg::{Catalog, CatalogBuilder, NamespaceIdent, TableIdent, table};
use iceberg_catalog_rest::RestCatalog;
use iceberg_datafusion::IcebergCatalogProvider;
use iceberg_storage_opendal::OpenDalStorageFactory;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::OnceCell;

pub struct Datalake {
    config: config::IcebergConfig,
    catalog: OnceCell<Arc<RestCatalog>>,
}
impl Default for Datalake {
    fn default() -> Self {
        Self {
            config: Default::default(),
            catalog: OnceCell::new(),
        }
    }
}

impl Datalake {
    // TODO: accept customized configuration
    pub fn new() -> Self {
        Self {
            config: IcebergConfig::default(),
            catalog: OnceCell::new(),
        }
    }

    pub async fn get_catalog(&self) -> crate::error::Result<Arc<RestCatalog>> {
        self.catalog
            .get_or_try_init(|| async {
                let builder = iceberg_catalog_rest::RestCatalogBuilder::default()
                    .with_storage_factory(Arc::new(OpenDalStorageFactory::S3 {
                        customized_credential_load: None,
                    }));
                let catalog = builder
                    .load("rest", self.config.to_properties())
                    .await
                    .map_err(|e| format!("build RestCatalog failed: {e}"))?;
                Ok::<_, crate::error::Error>(Arc::new(catalog))
            })
            .await?;
        Ok(self.catalog.get().unwrap().clone())
    }

    pub async fn list_all_tables(&self) -> crate::error::Result<Vec<(Vec<String>, String)>> {
        let catalog = self.get_catalog().await.unwrap();
        let namespaces = catalog.list_namespaces(None).await?;
        let mut result = Vec::new();

        for ns in &namespaces {
            let tables = catalog.list_tables(ns).await?;
            for t in tables {
                result.push((t.namespace.inner(), t.name));
            }
        }
        Ok(result)
    }

    pub async fn list_tables_in_namespace(
        &self,
        namespace: &NamespaceIdent,
    ) -> crate::error::Result<Vec<TableIdent>> {
        Ok(self.get_catalog().await?.list_tables(namespace).await?)
    }

    pub async fn get_provider(&self) -> crate::error::Result<IcebergCatalogProvider> {
        let rest_catalog = self.get_catalog().await?;

        let catalog_provider: IcebergCatalogProvider =
            IcebergCatalogProvider::try_new(rest_catalog.clone()).await?;
        Ok(catalog_provider)
    }

    pub async fn get_ctx(&self) -> crate::error::Result<SessionContext> {
        let rest_catalog = self.get_catalog().await?;

        let catalog_provider: IcebergCatalogProvider =
            IcebergCatalogProvider::try_new(rest_catalog.clone()).await?;

        let ctx = SessionContext::new();
        ctx.register_catalog("iceberg", Arc::new(catalog_provider));

        Ok(ctx)
    }

    pub async fn create_namespace_if_not_exist(
        &self,
        namespace: &NamespaceIdent,
    ) -> crate::error::Result<iceberg::Namespace> {
        let catalog = self.get_catalog().await?;
        let namespace_exist = catalog.namespace_exists(namespace).await?;
        if namespace_exist {
            let result_np: iceberg::Namespace = catalog.get_namespace(namespace).await?;
            Ok(result_np)
        } else {
            let result_np = catalog.create_namespace(namespace, HashMap::new()).await?;
            Ok(result_np)
        }
    }

    pub async fn create_table(
        &self,
        namespace: &NamespaceIdent,
        creation: iceberg::TableCreation,
    ) -> crate::error::Result<iceberg::table::Table> {
        let rest_catalog = self.get_catalog().await?;
        let table: table::Table = rest_catalog.create_table(namespace, creation).await?;
        Ok(table)
    }

    pub async fn create_table_if_not_exist(
        &self,
        namespace: &NamespaceIdent,
        creation: iceberg::TableCreation,
    ) -> crate::error::Result<iceberg::table::Table> {
        let rest_catalog = self.get_catalog().await?;
        let tableident: iceberg::TableIdent =
            iceberg::TableIdent::new(namespace.clone(), creation.name.clone());
        if rest_catalog.table_exists(&tableident).await? {
            Ok(rest_catalog.load_table(&tableident).await?)
        } else {
            let table: table::Table = rest_catalog.create_table(namespace, creation).await?;
            Ok(table)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_list_namespace() {
        let dk = Datalake::new();
        let namespace = &dk.list_all_tables().await.unwrap();
        println!("{:#?}", namespace);
        dbg!("hi");
    }
}
