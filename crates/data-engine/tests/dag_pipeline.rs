//! End-to-end DAG pipeline tests.
//!
//! These build a `DataEngine` with a plain `SessionContext` (no opendal/iceberg
//! backend) and exercise the scheduler through the three built-in node types:
//! `SourceNode` (load), `SqlNode` (transform), `SinkNode` (write).

use std::sync::Arc;

use data_engine::data_engine::{
    DataEngine, Sink, Source, WriteFormat,
    dag::{DagError, RuntimeStatus, SchedulerConfig},
    dag::graph::NamedDataFrames,
    nodes::{DagNode, NodeInput, NodeMeta},
};
use datafusion::common::HashMap;
use datafusion::prelude::SessionContext;

fn datasets_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_datasets")
}

#[tokio::test]
async fn insurance_pipeline_runs() {
    let ctx = Arc::new(SessionContext::new());
    let mut engine = DataEngine::new(ctx);
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
            "load",
        )
        .unwrap()
        .sql_node(
            "agg",
            "SELECT region, CAST(AVG(charges) AS BIGINT) AS avg_chg \
             FROM src GROUP BY region",
            "agg",
        )
        .unwrap()
        .sink_node(
            "out",
            Sink::File {
                path: out_path.into(),
                format: WriteFormat::Csv,
            },
        )
        .unwrap()
        .add_edge("load", "agg")
        .unwrap()
        .add_edge("agg", "out")
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
    // Source -> two independent SqlNodes (fan-out). Both should succeed.
    let ctx = Arc::new(SessionContext::new());
    let mut engine = DataEngine::new(ctx);
    let iris = datasets_dir().join("Iris.csv");

    engine
        .source_node(
            "load",
            Source::File {
                path: iris.to_str().unwrap().to_string(),
                format: None,
            },
            "load",
        )
        .unwrap()
        // Note: DataFusion lowercases unquoted identifiers, so quote the
        // mixed-case column name "Species".
        .sql_node(
            "setosa",
            r#"SELECT * FROM src WHERE "Species" = 'Iris-setosa'"#,
            "setosa",
        )
        .unwrap()
        .sql_node(
            "virginica",
            r#"SELECT * FROM src_2 WHERE "Species" = 'Iris-virginica'"#,
            "virginica",
        )
        .unwrap()
        .add_edge("load", "setosa")
        .unwrap()
        .add_edge("load", "virginica")
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
async fn bio_source_reads_vcf() {
    // SourceNode auto-detects the VCF.gz format from the extension.
    let ctx = Arc::new(SessionContext::new());
    let mut engine = DataEngine::new(ctx);
    let vcf = datasets_dir().join("test.vcf.gz");

    engine
        .source_node(
            "vcf",
            Source::File {
                path: vcf.to_str().unwrap().to_string(),
                format: None,
            },
            "vcf",
        )
        .unwrap();

    let report = engine.run().await.expect("run should succeed");
    assert!(report.ok);
    assert_eq!(report.status("vcf"), Some(RuntimeStatus::Success));
}

#[tokio::test]
async fn cycle_is_rejected() {
    let ctx = Arc::new(SessionContext::new());
    let mut engine = DataEngine::new(ctx);
    engine
        .sql_node("a", "SELECT 1", "a")
        .unwrap()
        .sql_node("b", "SELECT 1", "b")
        .unwrap()
        .add_edge("a", "b")
        .unwrap()
        .add_edge("b", "a")
        .unwrap();

    let err = engine.run().await.unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("cycle"), "{msg}");
}

/// A node that always fails — for the cascade test.
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
    async fn execute(&mut self, _inputs: &[NodeInput]) -> Result<NamedDataFrames, DagError> {
        Err(DagError::execution(
            "test.boom",
            std::io::Error::other("kaboom"),
        ))
    }
}

#[tokio::test]
async fn failure_cascades() {
    let ctx = Arc::new(SessionContext::new());
    let mut engine = DataEngine::new(ctx.clone());
    // Custom node: build meta in a separate statement (avoids self-borrow).
    let boom_meta = NodeMeta::new("boom");
    engine.add_node("boom", BoomNode(boom_meta)).unwrap();
    engine
        .sql_node("child", "SELECT 1", "child")
        .unwrap()
        .add_edge("boom", "child")
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

/// A node that sleeps — for the parallelism test.
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
    async fn execute(&mut self, _inputs: &[NodeInput]) -> Result<NamedDataFrames, DagError> {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        Ok(HashMap::new())
    }
}

#[tokio::test]
async fn scheduler_runs_in_parallel() {
    use std::time::{Duration, Instant};

    // 4 independent sleep nodes, concurrency 4 → ~100ms; concurrency 1 → ~400ms.
    for &concurrency in &[4usize, 1] {
        let ctx = Arc::new(SessionContext::new());
        let mut engine = DataEngine::new(ctx.clone()).with_config(SchedulerConfig {
            max_concurrency: concurrency,
        });
        for i in 0..4 {
            let id = format!("s{i}");
            let meta = NodeMeta::new(&id);
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
