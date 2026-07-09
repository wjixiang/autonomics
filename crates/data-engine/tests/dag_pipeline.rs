//! End-to-end DAG pipeline tests.
//!
//! These build a `DataEngine` with a plain `SessionContext` (no opendal/iceberg
//! backend) and exercise the scheduler through the three built-in node types:
//! `SourceNode` (load), `SqlNode` (transform), `SinkNode` (write).

use data_engine::data_engine::{
    DataEngine, Sink, Source, WriteFormat,
    dag::{DagError, RuntimeStatus, SchedulerConfig},
    dag::graph::NamedDataFrames,
    nodes::{DagNode, NodeInput, NodeMeta},
};
use datafusion::common::HashMap;

fn datasets_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_datasets")
}

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
        .add_edge("load", "agg", Some("src".to_string()))
        .unwrap()
        .add_edge("agg", "out", None)
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
    let mut engine = DataEngine::builder().build();
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
        .add_edge("load", "setosa", Some("src".to_string()))
        .unwrap()
        .add_edge("load", "virginica", Some("src_2".to_string()))
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
async fn fanout_auto_port() {
    // Same source → two SqlNodes using auto-generated port names.
    // Auto-generated ports are "{from}__{to}", so SQL must reference those names.
    let mut engine = DataEngine::builder().build();
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
        .sql_node(
            "setosa",
            r#"SELECT * FROM load__setosa WHERE "Species" = 'Iris-setosa'"#,
            "setosa",
        )
        .unwrap()
        .sql_node(
            "virginica",
            r#"SELECT * FROM load__virginica WHERE "Species" = 'Iris-virginica'"#,
            "virginica",
        )
        .unwrap()
        .add_edge("load", "setosa", None)
        .unwrap()
        .add_edge("load", "virginica", None)
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
    // Fan-out with concurrency=2 to exercise the concurrent registration path.
    // Both SqlNodes use explicit port names to avoid any name collision.
    let mut engine = DataEngine::builder().build().with_config(SchedulerConfig {
        max_concurrency: 2,
    });
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
        .sql_node(
            "branch_a",
            r#"SELECT COUNT(*) AS cnt FROM iris_a WHERE "Species" = 'Iris-setosa'"#,
            "branch_a",
        )
        .unwrap()
        .sql_node(
            "branch_b",
            r#"SELECT COUNT(*) AS cnt FROM iris_b WHERE "Species" = 'Iris-virginica'"#,
            "branch_b",
        )
        .unwrap()
        .add_edge("load", "branch_a", Some("iris_a".to_string()))
        .unwrap()
        .add_edge("load", "branch_b", Some("iris_b".to_string()))
        .unwrap();

    let report = engine.run().await.expect("run should succeed");
    assert!(
        report.ok,
        "statuses: {:?}; errors: {:?}",
        report.statuses, report.errors
    );
    for n in ["load", "branch_a", "branch_b"] {
        assert_eq!(report.status(n), Some(RuntimeStatus::Success), "{n}");
    }
}

#[tokio::test]
async fn bio_source_reads_vcf() {
    // SourceNode auto-detects the VCF.gz format from the extension.
    let mut engine = DataEngine::builder().build();
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
    let mut engine = DataEngine::builder().build();
    engine
        .sql_node("a", "SELECT 1", "a")
        .unwrap()
        .sql_node("b", "SELECT 1", "b")
        .unwrap()
        .add_edge("a", "b", None)
        .unwrap()
        .add_edge("b", "a", None)
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
        Err(DagError::Schedule("kaboom".into()))
    }
}

#[tokio::test]
async fn failure_cascades() {
    let mut engine = DataEngine::builder().build();
    // Custom node: build meta in a separate statement (avoids self-borrow).
    let boom_meta = NodeMeta::new("boom");
    engine.add_node("boom", BoomNode(boom_meta)).unwrap();
    engine
        .sql_node("child", "SELECT 1", "child")
        .unwrap()
        .add_edge("boom", "child", None)
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
        let mut engine = DataEngine::builder().build().with_config(SchedulerConfig {
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
