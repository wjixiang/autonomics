use std::sync::Arc;

use datafusion::{execution::object_store::ObjectStoreUrl, prelude::SessionContext};
use fs::OpendalFileStorage;

use crate::{DataSession, data_engine::dag::DAG};
pub mod dag;
pub mod nodes;

/// `DataEngine` is the core object that implements the data analysis engine.
/// It orchestrates ingestion, transformation, and querying of datasets
/// across the datalake.
pub struct DataEngine {
    // Data layer
    iceberg_session: Arc<DataSession>,
    file_session: Arc<OpendalFileStorage>,
    /// Connect to Iceberg and Opendal
    ctx: Arc<SessionContext>,

    dag: DAG,
}

impl DataEngine {
    pub fn new(data_session: Arc<DataSession>, file_session: Arc<OpendalFileStorage>) -> Self {
        let ctx = SessionContext::new();
        let object_url = ObjectStoreUrl::parse("file://").unwrap();
        ctx.register_object_store(object_url.as_ref(), file_session.clone());

        Self {
            iceberg_session: data_session,
            file_session,
            ctx: Arc::new(ctx),
            dag: DAG::default(),
        }
    }
    pub fn add_load_node(&mut self) {}
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::DataSession;
    use crate::data_engine::DataEngine;
    use datafusion::prelude::CsvReadOptions;
    use fs::OpendalFileStorage;

    #[tokio::test]
    async fn test_dataengine_opendal_datafusion() {
        let data_session = Arc::new(DataSession::new().await.unwrap());
        let file_session = Arc::new(OpendalFileStorage::new_in_memory());
        let test_data_file = std::fs::read("test_datasets/Iris.csv").unwrap();
        let write_res = file_session
            .op
            .write("/iris.csv", test_data_file)
            .await
            .unwrap();
        let engine = DataEngine::new(data_session, file_session);

        engine
            .ctx
            .register_csv("iris", "/iris.csv", CsvReadOptions::default())
            .await
            .unwrap();

        let df = engine.ctx.sql("SELECT * FROM iris LIMIT 5").await.unwrap();
        df.show().await.unwrap();
    }
}
