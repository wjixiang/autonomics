use std::sync::Arc;

use biofusion::ext::DataFusionReadExt;
use datafusion::{execution::object_store::ObjectStoreUrl, prelude::SessionContext};
use fs::OpendalFileStorage;

use crate::data_engine::dag::DAG;
use datalake::Datalake;

pub mod dag;
pub mod nodes;

/// `DataEngine` is the core object that implements the data analysis engine.
/// It orchestrates ingestion, transformation, and querying of datasets
/// across the datalake.
pub struct DataEngine {
    ctx: Arc<SessionContext>,
    dag: DAG,
}

impl DataEngine {
    pub fn new(ctx: Arc<SessionContext>) -> Self {
        Self {
            ctx,
            dag: DAG::default(),
        }
    }
    pub fn builder() -> DataEngineBuilder {
        DataEngineBuilder::new()
    }
    pub fn add_load_node(&mut self) {}
}

pub struct DataEngineBuilder {
    ctx: Arc<SessionContext>,
}

impl DataEngineBuilder {
    pub fn new() -> Self {
        DataEngineBuilder {
            ctx: Arc::new(SessionContext::new()),
        }
    }

    pub fn register_opendal_fs(self, file_session: Arc<OpendalFileStorage>) -> crate::Result<Self> {
        let object_url = ObjectStoreUrl::parse("file://")
            .map_err(|e| crate::Error::Custom(format!("cannot parse datafusion url: {e}")))?;
        self.ctx
            .register_object_store(object_url.as_ref(), file_session.clone());
        Ok(self)
    }

    pub async fn register_iceberg(self) -> crate::Result<Self> {
        let datalake = Datalake::default();
        let provider = datalake.get_provider().await?;
        self.ctx.register_catalog("iceberg", Arc::new(provider));

        Ok(self)
    }

    pub fn build(self) -> DataEngine {
        DataEngine::new(self.ctx)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::data_engine::DataEngine;
    use datafusion::prelude::CsvReadOptions;
    use fs::OpendalFileStorage;

    #[tokio::test]
    async fn test_dataengine_opendal_datafusion() {
        let file_session = Arc::new(OpendalFileStorage::new_in_fs());
        let test_data_file = std::fs::read("test_datasets/Iris.csv").unwrap();
        let _write_res = file_session
            .op
            .write("/iris.csv", test_data_file)
            .await
            .unwrap();
        let engine = DataEngine::builder()
            .register_opendal_fs(file_session)
            .unwrap()
            .register_iceberg()
            .await
            .unwrap()
            .build();

        engine
            .ctx
            .register_csv("iris", "/iris.csv", CsvReadOptions::default())
            .await
            .unwrap();

        let df = engine.ctx.sql("SELECT * FROM iris LIMIT 5").await.unwrap();
        df.clone().show().await.unwrap();
        let length = df.clone().count().await.unwrap();
        assert_eq!(length, 5);
    }
}
