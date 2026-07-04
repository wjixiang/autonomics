use std::sync::Arc;

use datafusion::{execution::object_store::ObjectStoreUrl, prelude::SessionContext};
use fs::OpendalFileStorage;

use crate::data_engine::dag::{DAG, Edge, RunReport, SchedulerConfig, run_dag};
use crate::data_engine::nodes::{DagNode, DagNodeStatus, NodeMeta};
use crate::error::Error;
use datalake::Datalake;

pub mod dag;
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

    /// Build a [`NodeMeta`] bound to this engine's shared context. Nodes must be
    /// constructed with this so they read through the registered stores/catalogs.
    pub fn node_meta(&self, id: impl Into<String>, name: impl Into<String>) -> NodeMeta {
        NodeMeta::new(
            id.into(),
            name.into(),
            DagNodeStatus::Idle,
            self.ctx.clone(),
        )
    }

    /// Register a node under `id`.
    ///
    /// When the node argument needs the engine's context, prefer the typed
    /// helpers ([`Self::source_node`], [`Self::sql_node`], [`Self::sink_node`])
    /// — they construct the `NodeMeta` internally so the call chains cleanly.
    /// With this raw `add_node`, build the meta in a separate statement to
    /// avoid a self-borrow conflict:
    ///
    /// ```ignore
    /// let meta = engine.node_meta("x", "x");
    /// engine.add_node("x", MyNode::new(meta, ...))?;
    /// ```
    pub fn add_node<N: DagNode + 'static>(
        &mut self,
        id: impl Into<String>,
        node: N,
    ) -> Result<&mut Self, Error> {
        self.dag.add_node(id.into(), Box::new(node))?;
        Ok(self)
    }

    /// Convenience: add a [`SourceNode`] (chaining-safe — meta built internally).
    pub fn source_node(
        &mut self,
        id: impl Into<String>,
        source: Source,
    ) -> Result<&mut Self, Error> {
        let id = id.into();
        let meta = self.node_meta(id.clone(), id.clone());
        self.dag
            .add_node(id, Box::new(SourceNode::new(meta, source)))?;
        Ok(self)
    }

    /// Convenience: add a [`SqlNode`] (chaining-safe).
    pub fn sql_node(
        &mut self,
        id: impl Into<String>,
        query: impl Into<String>,
    ) -> Result<&mut Self, Error> {
        let id = id.into();
        let meta = self.node_meta(id.clone(), id.clone());
        self.dag
            .add_node(id, Box::new(SqlNode::new(meta, query.into())))?;
        Ok(self)
    }

    /// Convenience: add a [`SinkNode`] (chaining-safe).
    pub fn sink_node(&mut self, id: impl Into<String>, sink: Sink) -> Result<&mut Self, Error> {
        let id = id.into();
        let meta = self.node_meta(id.clone(), id.clone());
        self.dag.add_node(id, Box::new(SinkNode::new(meta, sink)))?;
        Ok(self)
    }

    /// Add a dependency `from -> to`. The downstream node receives the upstream
    /// output under the default port name `"src"`.
    pub fn add_edge(
        &mut self,
        from: impl Into<String>,
        to: impl Into<String>,
    ) -> Result<&mut Self, Error> {
        let edge = Edge::new(from, to);
        self.dag.add_edge(edge)?;
        Ok(self)
    }

    /// Add a dependency with an explicit port name (e.g. the table name a
    /// `SqlNode` references).
    pub fn add_named_edge(
        &mut self,
        from: impl Into<String>,
        to: impl Into<String>,
        port: impl Into<String>,
    ) -> Result<&mut Self, Error> {
        let edge = Edge::new(from, to).with_port(port);
        self.dag.add_edge(edge)?;
        Ok(self)
    }

    /// Replace the scheduler configuration (concurrency, retry, …).
    pub fn with_config(mut self, config: SchedulerConfig) -> Self {
        self.config = config;
        self
    }

    /// Validate and run every node of the DAG.
    pub async fn run(&mut self) -> Result<RunReport, Error> {
        let report = run_dag(&mut self.dag, &self.config).await?;
        Ok(report)
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
