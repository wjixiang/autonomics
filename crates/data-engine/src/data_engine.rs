use std::sync::Arc;

use datafusion::{execution::object_store::ObjectStoreUrl, prelude::SessionContext};
use fs::OpendalFileStorage;

use crate::data_engine::dag::{DAG, RunReport, SchedulerConfig};
use crate::data_engine::error::{Error, Result};
use crate::data_engine::nodes::{DagNode, NodeMeta};
use datalake::Datalake;

pub mod dag;
pub mod error;
pub mod nodes;

pub use nodes::{FileFormat, Sink, SinkNode, Source, SourceNode, SqlNode, WriteFormat};

/// `DataEngine` is the core object that implements the data analysis engine.
/// It orchestrates ingestion, transformation, and querying of datasets via a
/// [`DAG`] of nodes executed by an async scheduler.
pub struct DataEngine {
    ctx: Arc<SessionContext>,
    dag: DAG,
    config: SchedulerConfig,
}

impl DataEngine {
    pub fn new(ctx: Arc<SessionContext>) -> Self {
        Self {
            ctx,
            dag: DAG::default(),
            config: SchedulerConfig::default(),
        }
    }

    pub fn builder() -> DataEngineBuilder {
        DataEngineBuilder::new()
    }

    /// Returns the shared session context (object stores, catalogs, …).
    pub fn ctx(&self) -> Arc<SessionContext> {
        self.ctx.clone()
    }

    /// Build a [`NodeMeta`] with the given id.
    fn crate_node_meta(&self, id: impl Into<String>) -> NodeMeta {
        NodeMeta::new(id.into())
    }

    /// Register a node under `id`.
    ///
    /// Prefer the typed helpers ([`Self::source_node`], [`Self::sql_node`],
    /// [`Self::sink_node`]) — they construct the `NodeMeta` internally so the
    /// call chains cleanly. With this raw `add_node`, build the meta in a
    /// separate statement:
    ///
    /// ```ignore
    /// let meta = NodeMeta::new("x");
    /// engine.add_node("x", MyNode::new(meta, ...))?;
    /// ```
    pub fn add_node<N: DagNode + 'static>(
        &mut self,
        id: impl Into<String>,
        node: N,
    ) -> Result<&mut Self> {
        self.dag.add_node(id.into(), Box::new(node))?;
        Ok(self)
    }

    pub fn remove_node(&mut self, id: impl Into<String>) -> Result<&mut Self> {
        let id = id.into();
        self.dag.delete_node(&id)?;
        Ok(self)
    }

    /// Convenience: add a [`SourceNode`] (chaining-safe — meta built internally).
    pub fn source_node(
        &mut self,
        id: impl Into<String>,
        source: Source,
        output_df_name: impl Into<String>,
    ) -> Result<&mut Self> {
        let id = id.into();
        let output_df_name = output_df_name.into();
        let meta = self.crate_node_meta(id.clone());
        self.dag.add_node(
            id,
            Box::new(SourceNode::new(
                meta,
                source,
                self.ctx.clone(),
                output_df_name,
            )),
        )?;
        Ok(self)
    }

    /// Convenience: add a [`SqlNode`] (chaining-safe).
    pub fn sql_node(
        &mut self,
        id: impl Into<String>,
        query: impl Into<String>,
        output_df_name: impl Into<String>,
    ) -> Result<&mut Self> {
        let id = id.into();
        let output_df_name = output_df_name.into();
        let meta = self.crate_node_meta(id.clone());
        self.dag.add_node(
            id,
            Box::new(SqlNode::new(
                meta,
                query.into(),
                self.ctx.clone(),
                output_df_name,
            )),
        )?;
        Ok(self)
    }

    /// Convenience: add a [`SinkNode`] (chaining-safe).
    pub fn sink_node(&mut self, id: impl Into<String>, sink: Sink) -> Result<&mut Self> {
        let id = id.into();
        let meta = self.crate_node_meta(id.clone());
        self.dag.add_node(id, Box::new(SinkNode::new(meta, sink)))?;
        Ok(self)
    }

    /// Add a dependency `from -> to`. The downstream node receives the upstream
    /// output registered under the upstream node's output DataFrame name.
    pub fn add_edge(
        &mut self,
        from: impl Into<String>,
        to: impl Into<String>,
    ) -> Result<&mut Self> {
        self.dag.add_edge(from, to)?;
        Ok(self)
    }

    /// Replace the scheduler configuration (concurrency, retry, …).
    pub fn with_config(mut self, config: SchedulerConfig) -> Self {
        self.config = config;
        self
    }

    /// Validate and run every node of the DAG.
    pub async fn run(&mut self) -> Result<RunReport> {
        Ok(self.dag.run(&self.config).await?)
    }

    pub async fn get_output(
        &self,
        node_id: impl Into<String>,
    ) -> Option<crate::data_engine::dag::graph::NamedDataFrames> {
        self.dag.output(node_id.into().as_ref())
    }
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

    pub fn register_opendal_fs(self, file_session: Arc<OpendalFileStorage>) -> Result<Self> {
        let object_url = ObjectStoreUrl::parse("file://")
            .map_err(|e| Error::Custom(format!("cannot parse datafusion url: {e}")))?;
        self.ctx
            .register_object_store(object_url.as_ref(), file_session.clone());
        Ok(self)
    }

    pub async fn register_iceberg(self) -> Result<Self> {
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
        let file_session = Arc::new(OpendalFileStorage::new("/mnt/disk3/test"));
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
