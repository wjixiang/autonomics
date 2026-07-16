use std::sync::Arc;

use datafusion::{execution::object_store::ObjectStoreUrl, prelude::SessionContext};
use fs::OpendalFileStorage;
use iceberg::Catalog;
use iceberg_catalog_rest::RestCatalog;

use crate::data_engine::dag::{DAG, RunReport, SchedulerConfig};
use crate::data_engine::error::{Error, Result};
use crate::data_engine::nodes::DagNode;
use datalake::Datalake;

pub mod dag;
pub mod error;
pub mod nodes;

pub use nodes::{
    FileFormat, LdscHsqConfig, LdscHsqNode, LinearRegressionNode, Sink, SinkMode, SinkNode, Source,
    SourceNode, SqlNode, WriteFormat,
};

/// Convenience alias for the default engine backed by a REST Iceberg catalog.
pub type IcebergDataEngine = DataEngine<RestCatalog>;

/// `DataEngine` is the core object that implements the data analysis engine.
/// It orchestrates ingestion, transformation, and querying of datasets via a
/// [`DAG`] of nodes executed by an async scheduler.
pub struct DataEngine<R: Catalog> {
    ctx: SessionContext,
    catalog: Option<Arc<R>>,
    dag: DAG,
    config: SchedulerConfig,
}

impl<R: Catalog> DataEngine<R> {
    pub fn new(ctx: SessionContext, catalog: Option<Arc<R>>) -> Self {
        Self {
            ctx,
            dag: DAG::default(),
            config: SchedulerConfig::default(),
            catalog,
        }
    }

    /// Returns the Iceberg catalog, if registered.
    pub fn catalog(&self) -> Option<Arc<R>> {
        self.catalog.clone()
    }
}

// builder() lives in a concrete impl so callers don't need to specify <R>.
impl DataEngine<RestCatalog> {
    pub fn builder() -> DataEngineBuilder {
        DataEngineBuilder::default()
    }
}

impl<R: Catalog> DataEngine<R> {
    /// Returns the shared session context (object stores, catalogs, …).
    pub fn ctx(&self) -> SessionContext {
        self.ctx.clone()
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

    pub fn view_dag(&self) -> Result<String> {
        Ok(self.dag.to_dot())
    }

    /// Clear all nodes, edges, and runtime state — start fresh.
    pub fn clear_dag(&mut self) -> Result<()> {
        self.dag.clear();
        Ok(())
    }

    /// Convenience: add a [`SourceNode`] (chaining-safe — meta built internally).
    pub fn source_node(&mut self, id: impl Into<String>, source: Source) -> Result<&mut Self> {
        let id = id.into();
        self.dag.add_node(
            id.clone(),
            Box::new(SourceNode::new(id, source, self.ctx.clone())),
        )?;
        Ok(self)
    }

    /// Convenience: add a [`SqlNode`] (chaining-safe).
    pub fn sql_node(
        &mut self,
        id: impl Into<String>,
        query: impl Into<String>,
    ) -> Result<&mut Self> {
        let id = id.into();
        self.dag.add_node(
            id.clone(),
            Box::new(SqlNode::new(id, query.into(), self.ctx.clone())),
        )?;
        Ok(self)
    }

    /// Convenience: add a [`SinkNode`] (chaining-safe).
    pub fn sink_node(
        &mut self,
        id: impl Into<String>,
        sink: Sink,
        mode: SinkMode,
        datalake: Arc<Datalake>,
    ) -> Result<&mut Self> {
        let id = id.into();
        self.dag.add_node(
            id.clone(),
            Box::new(SinkNode::new(id, sink, mode, self.ctx.clone(), datalake)),
        )?;
        Ok(self)
    }

    /// Convenience: add a [`LinearRegressionNode`] (chaining-safe).
    ///
    /// Fits an OLS regression of `y_column` on `x_columns` over the input
    /// DataFrame produced by upstream nodes. When `intercept` is true, the
    /// model includes an intercept term (reported as the first row).
    pub fn linear_regression_node(
        &mut self,
        id: impl Into<String>,
        x_columns: Vec<String>,
        y_column: impl Into<String>,
        intercept: bool,
    ) -> Result<&mut Self> {
        let id = id.into();
        self.dag.add_node(
            id.clone(),
            Box::new(LinearRegressionNode::new(
                id,
                x_columns,
                y_column.into(),
                intercept,
            )),
        )?;
        Ok(self)
    }

    /// Convenience: add a [`LdscHsqNode`] (chaining-safe).
    ///
    /// Runs LD Score Regression for SNP-heritability (h2). Accepts raw GWAS
    /// summary statistics as input, queries the Iceberg data lake for LD score
    /// panel data (`genetics.ld_score.{ld_score_table}`), joins on rsid, and
    /// runs LDSC internally.
    pub fn ldsc_node(
        &mut self,
        id: impl Into<String>,
        datalake: Arc<Datalake>,
        z_column: String,
        n_column: String,
        rsid_column: String,
        ldsc: LdscHsqConfig,
    ) -> Result<&mut Self> {
        let id = id.into();
        self.dag.add_node(
            id.clone(),
            Box::new(LdscHsqNode::new(
                id,
                datalake,
                z_column,
                n_column,
                rsid_column,
                ldsc,
            )),
        )?;
        Ok(self)
    }

    /// Add an edge from `from`'s default output port to `to`'s default input
    /// port (convenience form for single-port nodes).
    pub fn add_edge(
        &mut self,
        from: impl Into<String>,
        to: impl Into<String>,
        from_port: u8,
        to_port: u8,
    ) -> Result<&mut Self> {
        self.dag.add_edge(from, to, from_port, to_port)?;
        Ok(self)
    }

    // /// Add an edge connecting `from`'s `from_port` output port to `to`'s
    // /// `to_port` input port.
    // pub fn add_edge_port(
    //     &mut self,
    //     from: impl Into<String>,
    //     from_port: impl Into<String>,
    //     to: impl Into<String>,
    //     to_port: impl Into<String>,
    // ) -> Result<&mut Self> {
    //     self.dag.add_edge_port(from, from_port, to, to_port)?;
    //     Ok(self)
    // }
    //
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
    ) -> Option<crate::data_engine::dag::graph::PortOutputs> {
        self.dag.output(node_id.into().as_ref())
    }
}

pub struct DataEngineBuilder {
    ctx: SessionContext,
    catalog: Option<Arc<RestCatalog>>,
}

impl Default for DataEngineBuilder {
    fn default() -> Self {
        Self {
            ctx: SessionContext::new(),
            catalog: None,
        }
    }
}

impl DataEngineBuilder {
    pub fn register_opendal_fs(self, file_session: Arc<OpendalFileStorage>) -> Result<Self> {
        let object_url = ObjectStoreUrl::parse("file://")
            .map_err(|e| Error::Custom(format!("cannot parse datafusion url: {e}")))?;
        self.ctx
            .register_object_store(object_url.as_ref(), file_session.clone());
        Ok(self)
    }

    pub async fn register_iceberg(mut self) -> Result<Self> {
        let datalake = Datalake::default();
        let provider = datalake.get_provider().await?;
        self.ctx.register_catalog("iceberg", Arc::new(provider));
        self.catalog = Some(datalake.get_catalog().await?);

        Ok(self)
    }

    pub fn build(self) -> IcebergDataEngine {
        DataEngine::new(self.ctx, self.catalog)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{DataEngine, Sink, SinkMode, Source, WriteFormat};
    use crate::data_engine::dag::graph::PortOutputs;
    use crate::data_engine::dag::{DagError, RuntimeStatus, SchedulerConfig};
    use crate::data_engine::error::Error;
    use crate::data_engine::nodes::{DagNode, NodeInput, NodeMeta};
    use datafusion::common::HashMap;
    use datafusion::prelude::CsvReadOptions;
    use datalake::Datalake;
    use fs::OpendalFileStorage;

    fn datasets_dir() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_datasets")
    }

    // ── Existing opendal+iceberg integration test ──────────────────────

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

    // ── DAG pipeline tests (migrated from tests/dag_pipeline.rs) ────────

    #[tokio::test]
    async fn insurance_pipeline_runs() {
        let mut engine = DataEngine::builder().build();
        let csv_path = datasets_dir().join("insurance.csv");
        let out_path = "/tmp/dag_insurance_out.csv";
        let _ = std::fs::remove_file(out_path);

        engine
            .source_node(
                "load",
                Source::File {
                    path: csv_path.to_str().unwrap().to_string(),
                    format: None,
                },
            )
            .unwrap()
            .sql_node(
                "agg",
                // agg's single input (port 0) is registered as "port_0".
                "SELECT region, CAST(AVG(charges) AS BIGINT) AS avg_chg \
                 FROM port_0 GROUP BY region",
            )
            .unwrap()
            .sink_node(
                "out",
                Sink::File {
                    path: out_path.into(),
                    format: WriteFormat::Csv,
                },
                SinkMode::Overwrite,
                Arc::new(Datalake::default()),
            )
            .unwrap()
            // Default edges: each node has a single relevant port, resolved automatically.
            .add_edge("load", "agg", 0, 0)
            .unwrap()
            .add_edge("agg", "out", 0, 0)
            .unwrap();

        let report = engine.run().await.expect("run should succeed");
        assert!(report.ok, "all nodes should succeed: {:?}", report.statuses);
        for n in ["load", "agg", "out"] {
            assert_eq!(report.status(n), Some(RuntimeStatus::Success), "{n}");
        }

        // 4 regions in the insurance dataset → 4 data rows + 1 header.
        let lines = std::fs::read_to_string(out_path).unwrap();
        assert_eq!(lines.lines().count(), 5, "expected 4 region rows + header");
    }

    #[tokio::test]
    async fn fanout_branch() {
        // One source → two independent SqlNodes (fan-out from one output port).
        let mut engine = DataEngine::builder().build();
        let iris = datasets_dir().join("Iris.csv");

        engine
            .source_node(
                "load",
                Source::File {
                    path: iris.to_str().unwrap().to_string(),
                    format: None,
                },
            )
            .unwrap()
            // Note: DataFusion lowercases unquoted identifiers, so quote the
            // mixed-case column name "Species".
            .sql_node(
                "setosa",
                r#"SELECT * FROM port_0 WHERE "Species" = 'Iris-setosa'"#,
            )
            .unwrap()
            .sql_node(
                "virginica",
                r#"SELECT * FROM port_0 WHERE "Species" = 'Iris-virginica'"#,
            )
            .unwrap()
            .add_edge("load", "setosa", 0, 0)
            .unwrap()
            .add_edge("load", "virginica", 0, 0)
            .unwrap();

        let report = engine.run().await.expect("run should succeed");
        assert!(
            report.ok,
            "statuses: {:?}; errors: {:?}",
            report.statuses, report.errors
        );
        for n in ["load", "setosa", "virginica"] {
            assert_eq!(report.status(n), Some(RuntimeStatus::Success), "{n}");
        }
    }

    #[tokio::test]
    async fn fanout_concurrent() {
        // Fan-out under concurrency=2 — verifies no SessionContext registration
        // collision between concurrent consumers of the same source port.
        let mut engine = DataEngine::builder()
            .build()
            .with_config(SchedulerConfig {
                max_concurrency: 2,
                ..SchedulerConfig::default()
            });
        let iris = datasets_dir().join("Iris.csv");

        engine
            .source_node(
                "load",
                Source::File {
                    path: iris.to_str().unwrap().to_string(),
                    format: None,
                },
            )
            .unwrap()
            .sql_node(
                "a",
                r#"SELECT COUNT(*) AS cnt FROM port_0 WHERE "Species" = 'Iris-setosa'"#,
            )
            .unwrap()
            .sql_node(
                "b",
                r#"SELECT COUNT(*) AS cnt FROM port_0 WHERE "Species" = 'Iris-virginica'"#,
            )
            .unwrap()
            .add_edge("load", "a", 0, 0)
            .unwrap()
            .add_edge("load", "b", 0, 0)
            .unwrap();

        let report = engine.run().await.expect("run should succeed");
        assert!(
            report.ok,
            "statuses: {:?}; errors: {:?}",
            report.statuses, report.errors
        );
        for n in ["load", "a", "b"] {
            assert_eq!(report.status(n), Some(RuntimeStatus::Success), "{n}");
        }
    }

    /// A multi-input join: two sources feed a single SqlNode's `left`/`right`
    /// input ports, exercising named ports and `add_edge_port`.
    #[tokio::test]
    async fn join_named_ports() {
        let mut engine = DataEngine::builder().build();
        let iris = datasets_dir().join("Iris.csv");

        engine
            .source_node(
                "src_a",
                Source::File {
                    path: iris.to_str().unwrap().to_string(),
                    format: None,
                },
            )
            .unwrap()
            .source_node(
                "src_b",
                Source::File {
                    path: iris.to_str().unwrap().to_string(),
                    format: None,
                },
            )
            .unwrap();

        // A join node with two named input ports. Inputs are registered as
        // "port_0" and "port_1" by the SqlNode (one table per upstream port).
        let join_meta = NodeMeta::new("join")
            .add_input_port(None)
            .add_input_port(None)
            .add_output_port(None);
        engine
            .add_node(
                "join",
                super::SqlNode::from_meta(
                    join_meta,
                    r#"SELECT COUNT(*) AS cnt FROM port_0"#.to_string(),
                    engine.ctx(),
                    // "result".to_string(),
                ),
            )
            .unwrap()
            .sink_node(
                "out",
                Sink::File {
                    path: "/tmp/dag_join_out.csv".into(),
                    format: WriteFormat::Csv,
                },
                SinkMode::Overwrite,
                Arc::new(Datalake::default()),
            )
            .unwrap()
            .add_edge("src_a", "join", 0, 0)
            .unwrap()
            .add_edge("src_b", "join", 0, 1)
            .unwrap()
            .add_edge("join", "out", 0, 0)
            .unwrap();

        let report = engine.run().await.expect("run should succeed");
        assert!(
            report.ok,
            "statuses: {:?}; errors: {:?}",
            report.statuses, report.errors
        );
        for n in ["src_a", "src_b", "join", "out"] {
            assert_eq!(report.status(n), Some(RuntimeStatus::Success), "{n}");
        }
    }

    #[tokio::test]
    async fn bio_source_reads_vcf() {
        // SourceNode auto-detects the VCF.gz format from the extension.
        let mut engine = DataEngine::builder().build();
        let vcf = datasets_dir().join("sample.vcf.gz");

        engine
            .source_node(
                "vcf",
                Source::File {
                    path: vcf.to_str().unwrap().to_string(),
                    format: None,
                },
            )
            .unwrap();

        let report = engine.run().await.expect("run should succeed");
        assert!(report.ok);
        assert_eq!(report.status("vcf"), Some(RuntimeStatus::Success));
    }

    /// Regression: with default `compute_row_counts = false`, running a
    /// source-only VCF DAG must NOT eagerly `count()` the dataset. The
    /// `output_rows` field of the report should stay `None`, which proves the
    /// eager COUNT(*) scan never ran.
    ///
    /// Before the fix, `build_node_reports` unconditionally called `df.count()`
    /// on every output, forcing a full decompression + parse of the VCF — the
    /// worst-case behavior for "source node, no downstream consumer".
    #[tokio::test]
    async fn vcf_source_skips_count_by_default() {
        let mut engine = DataEngine::builder().build();
        let vcf = datasets_dir().join("sample.vcf.gz");

        engine
            .source_node(
                "vcf",
                Source::File {
                    path: vcf.to_str().unwrap().to_string(),
                    format: None,
                },
            )
            .unwrap();

        let report = engine.run().await.expect("run should succeed");
        assert!(report.ok);
        assert_eq!(report.status("vcf"), Some(RuntimeStatus::Success));

        let node = report
            .nodes
            .iter()
            .find(|n| n.id == "vcf")
            .expect("source node should be in the report");

        // The whole point of the fix: default config must NOT compute rows.
        assert!(
            node.output_rows.is_none(),
            "default config should skip row counting; got {:?}",
            node.output_rows
        );
    }

    /// Opt-in path: when the caller explicitly sets
    /// `compute_row_counts = true`, the report must contain a row count for
    /// every successful source node — proving that the parallel `join_all`
    /// path works end-to-end.
    #[tokio::test]
    async fn vcf_source_computes_rows_when_opted_in() {
        let mut engine = DataEngine::builder()
            .build()
            .with_config(SchedulerConfig {
                compute_row_counts: true,
                ..SchedulerConfig::default()
            });
        let vcf = datasets_dir().join("sample.vcf.gz");

        engine
            .source_node(
                "vcf",
                Source::File {
                    path: vcf.to_str().unwrap().to_string(),
                    format: None,
                },
            )
            .unwrap();

        let report = engine.run().await.expect("run should succeed");
        assert!(report.ok);

        let node = report
            .nodes
            .iter()
            .find(|n| n.id == "vcf")
            .expect("source node should be in the report");

        let rows = node
            .output_rows
            .expect("opted-in run must populate row counts");
        assert!(
            rows > 0,
            "sample VCF should report a positive row count, got {rows}"
        );

        // Output schema should still be populated regardless of the count
        // setting — `schema()` inspects the LogicalPlan only, not data.
        assert!(
            node.output_schema.is_some(),
            "schema should be visible from the LogicalPlan without execution"
        );
    }

    #[tokio::test]
    async fn cycle_is_rejected() {
        let mut engine = DataEngine::builder().build();
        engine
            .sql_node("a", "SELECT 1")
            .unwrap()
            .sql_node("b", "SELECT 1")
            .unwrap()
            .add_edge("a", "b", 0, 0)
            .unwrap()
            .add_edge("b", "a", 0, 0)
            .unwrap();

        let err = engine.run().await.unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("cycle"), "{msg}");
    }

    #[tokio::test]
    async fn disconnected_input_port_rejected() {
        // A fixed-input node with no incoming edge → validation must reject the
        // dangling input port. LinearRegressionNode declares exactly one required
        // (fixed) input port. Variadic nodes like SqlNode are exempt: they
        // declare no ports and accept any number of inputs, so this check only
        // applies to fixed-input nodes. Validation runs before execution, so the
        // placeholder column names never get used.
        let mut engine = DataEngine::builder().build();
        engine
            .linear_regression_node("lr", vec!["x".into()], "y", true)
            .unwrap();

        let err = engine.run().await.unwrap_err();
        assert!(
            matches!(err, Error::Dag(DagError::PortDisconnected { ref node, .. }) if node == "lr"),
            "expected PortDisconnected, got {err:?}"
        );
    }

    #[tokio::test]
    async fn overconnected_input_port_rejected() {
        // Two edges into one input port (strict 1:1 violation).
        let mut engine = DataEngine::builder().build();
        let iris = datasets_dir().join("Iris.csv");
        engine
            .source_node(
                "s1",
                Source::File {
                    path: iris.to_str().unwrap().to_string(),
                    format: None,
                },
            )
            .unwrap()
            .source_node(
                "s2",
                Source::File {
                    path: iris.to_str().unwrap().to_string(),
                    format: None,
                },
            )
            .unwrap()
            .sql_node("c", "SELECT 1")
            .unwrap()
            .add_edge("s1", "c", 0, 0)
            .unwrap()
            .add_edge("s2", "c", 0, 0)
            .unwrap();

        let err = engine.run().await.unwrap_err();
        assert!(
            matches!(err, Error::Dag(DagError::PortOverconnected { ref node, .. }) if node == "c"),
            "expected PortOverconnected, got {err:?}"
        );
    }

    #[tokio::test]
    async fn unknown_port_rejected() {
        let mut engine = DataEngine::builder().build();
        engine
            .sql_node("a", "SELECT 1")
            .unwrap()
            .sql_node("b", "SELECT 1")
            .unwrap()
            // "a" has no output port named "nope".
            .add_edge("a", "b", 99, 0)
            .unwrap();

        let err = engine.run().await.unwrap_err();
        assert!(
            matches!(err, Error::Dag(DagError::PortNotFound { ref node, direction: "output", .. }) if node == "a"),
            "expected PortNotFound(output), got {err:?}"
        );
    }

    /// A node that always fails — for the cascade test. Source-like (no inputs).
    #[derive(Clone)]
    struct BoomNode(NodeMeta);
    #[async_trait::async_trait]
    impl DagNode for BoomNode {
        fn meta(&self) -> &NodeMeta {
            &self.0
        }
        fn clone_box(&self) -> Box<dyn DagNode> {
            Box::new((*self).clone())
        }
        fn node_type(&self) -> &str {
            "boom"
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        async fn execute(&mut self, _inputs: &[NodeInput]) -> Result<PortOutputs, DagError> {
            Err(DagError::Schedule("kaboom".into()))
        }
    }

    #[tokio::test]
    async fn failure_cascades() {
        let mut engine = DataEngine::builder().build();
        // Source-like boom node (no input ports) so it passes port validation.
        let boom_meta = NodeMeta::source("boom");
        engine.add_node("boom", BoomNode(boom_meta)).unwrap();
        engine
            .sql_node("child", "SELECT 1")
            .unwrap()
            .add_edge("boom", "child", 0, 0)
            .unwrap();

        let report = engine.run().await.expect("run completes even on failure");
        assert!(!report.ok, "run should report failure");
        assert_eq!(report.status("boom"), Some(RuntimeStatus::Failed));
        assert_eq!(
            report.status("child"),
            Some(RuntimeStatus::Skipped),
            "descendant of a failed node must be skipped"
        );
    }

    /// A node that sleeps — for the parallelism test. No inputs, no real output.
    #[derive(Clone)]
    struct SleepNode(NodeMeta);
    #[async_trait::async_trait]
    impl DagNode for SleepNode {
        fn meta(&self) -> &NodeMeta {
            &self.0
        }
        fn clone_box(&self) -> Box<dyn DagNode> {
            Box::new((*self).clone())
        }
        fn node_type(&self) -> &str {
            "sleep"
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        async fn execute(&mut self, _inputs: &[NodeInput]) -> Result<PortOutputs, DagError> {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            Ok(HashMap::new())
        }
    }

    #[tokio::test]
    async fn scheduler_runs_in_parallel() {
        use std::time::{Duration, Instant};

        // 4 independent sleep nodes, concurrency 4 → ~100ms; concurrency 1 → ~400ms.
        for &concurrency in &[4usize, 1] {
            let mut engine = DataEngine::builder().build().with_config(SchedulerConfig {
                max_concurrency: concurrency,
                ..SchedulerConfig::default()
            });
            for i in 0..4 {
                let id = format!("s{i}");
                // No input ports: standalone nodes pass port validation.
                let meta = NodeMeta::source(&id);
                engine.add_node(id, SleepNode(meta)).unwrap();
            }
            let start = Instant::now();
            let report = engine.run().await.unwrap();
            let elapsed = start.elapsed();
            assert!(report.ok);
            assert_eq!(report.status("s0"), Some(RuntimeStatus::Success));
            if concurrency == 4 {
                assert!(
                    elapsed < Duration::from_millis(350),
                    "parallel run took too long: {elapsed:?}"
                );
            } else {
                assert!(
                    elapsed >= Duration::from_millis(350),
                    "serial run was too fast: {elapsed:?}"
                );
            }
        }
    }
}
