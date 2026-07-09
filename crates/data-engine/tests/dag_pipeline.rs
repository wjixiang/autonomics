//! End-to-end DAG pipeline tests for the port-based engine.
//!
//! These build a `DataEngine` with a plain `SessionContext` (no opendal/iceberg
//! backend) and exercise the scheduler through the built-in node types. Each
//! upstream DataFrame is registered in the shared `SessionContext` under the
//! globally-unique name `"{consuming_node}__{input_port}"`, so SQL references
//! its own inputs as `FROM {self}__{port}`.

use data_engine::data_engine::{
    DataEngine, Sink, Source, WriteFormat,
    dag::graph::NamedDataFrames,
    dag::{DagError, RuntimeStatus, SchedulerConfig},
    error::Error,
    nodes::{DagNode, NodeInput, NodeMeta, Port},
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
            // agg's single default input is registered as "agg__default".
            "SELECT region, CAST(AVG(charges) AS BIGINT) AS avg_chg \
             FROM agg__default GROUP BY region",
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
        // Default edges: each node has a single relevant port, resolved automatically.
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
            "load",
        )
        .unwrap()
        // Note: DataFusion lowercases unquoted identifiers, so quote the
        // mixed-case column name "Species".
        .sql_node(
            "setosa",
            r#"SELECT * FROM setosa__default WHERE "Species" = 'Iris-setosa'"#,
            "setosa",
        )
        .unwrap()
        .sql_node(
            "virginica",
            r#"SELECT * FROM virginica__default WHERE "Species" = 'Iris-virginica'"#,
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
async fn fanout_concurrent() {
    // Fan-out under concurrency=2 — verifies no SessionContext registration
    // collision between concurrent consumers of the same source port.
    let mut engine = DataEngine::builder()
        .build()
        .with_config(SchedulerConfig { max_concurrency: 2 });
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
            "a",
            r#"SELECT COUNT(*) AS cnt FROM a__default WHERE "Species" = 'Iris-setosa'"#,
            "a",
        )
        .unwrap()
        .sql_node(
            "b",
            r#"SELECT COUNT(*) AS cnt FROM b__default WHERE "Species" = 'Iris-virginica'"#,
            "b",
        )
        .unwrap()
        .add_edge("load", "a")
        .unwrap()
        .add_edge("load", "b")
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
            "data",
        )
        .unwrap()
        .source_node(
            "src_b",
            Source::File {
                path: iris.to_str().unwrap().to_string(),
                format: None,
            },
            "data",
        )
        .unwrap();

    // A join node with two named input ports. Inputs are registered as
    // "join__left" and "join__right" in the shared SessionContext.
    let join_meta = NodeMeta::new("join").with_inputs(vec![Port::new("left"), Port::new("right")]);
    engine
        .add_node(
            "join",
            data_engine::data_engine::SqlNode::new(
                join_meta,
                r#"SELECT COUNT(*) AS cnt FROM join__left"#.to_string(),
                engine.ctx(),
                "result".to_string(),
            ),
        )
        .unwrap()
        .sink_node(
            "out",
            Sink::File {
                path: "/tmp/dag_join_out.csv".into(),
                format: WriteFormat::Csv,
            },
        )
        .unwrap()
        .add_edge_port("src_a", "data", "join", "left")
        .unwrap()
        .add_edge_port("src_b", "data", "join", "right")
        .unwrap()
        .add_edge("join", "out")
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
        .add_edge("a", "b")
        .unwrap()
        .add_edge("b", "a")
        .unwrap();

    let err = engine.run().await.unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("cycle"), "{msg}");
}

#[tokio::test]
async fn disconnected_input_port_rejected() {
    // A SqlNode with no incoming edge → validation must reject the dangling
    // default input port.
    let mut engine = DataEngine::builder().build();
    engine.sql_node("orphan", "SELECT 1", "out").unwrap();

    let err = engine.run().await.unwrap_err();
    assert!(
        matches!(err, Error::Dag(DagError::PortDisconnected { ref node, .. }) if node == "orphan"),
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
            "d",
        )
        .unwrap()
        .source_node(
            "s2",
            Source::File {
                path: iris.to_str().unwrap().to_string(),
                format: None,
            },
            "d",
        )
        .unwrap()
        .sql_node("c", "SELECT 1", "o")
        .unwrap()
        .add_edge("s1", "c")
        .unwrap()
        .add_edge("s2", "c")
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
        .sql_node("a", "SELECT 1", "o")
        .unwrap()
        .sql_node("b", "SELECT 1", "o")
        .unwrap()
        // "a" has no output port named "nope".
        .add_edge_port("a", "nope", "b", "default")
        .unwrap();

    let err = engine.run().await.unwrap_err();
    assert!(
        matches!(err, Error::Dag(DagError::PortNotFound { ref node, direction: "output", .. }) if node == "a"),
        "expected PortNotFound(output), got {err:?}"
    );
}

#[tokio::test]
async fn ambiguous_default_edge_rejected() {
    // A node with two input ports cannot be connected with the default add_edge.
    let mut engine = DataEngine::builder().build();
    let iris = datasets_dir().join("Iris.csv");
    engine
        .source_node(
            "s",
            Source::File {
                path: iris.to_str().unwrap().to_string(),
                format: None,
            },
            "d",
        )
        .unwrap();
    let join_meta = NodeMeta::new("j").with_inputs(vec![Port::new("left"), Port::new("right")]);
    engine
        .add_node(
            "j",
            data_engine::data_engine::SqlNode::new(
                join_meta,
                "SELECT 1".to_string(),
                engine.ctx(),
                "o".to_string(),
            ),
        )
        .unwrap();
    // Default edge into a 2-input node → AmbiguousPort at add_edge time.
    match engine.add_edge("s", "j") {
        Err(Error::Dag(DagError::AmbiguousPort { ref node })) if node == "j" => {}
        Err(e) => panic!("expected AmbiguousPort, got {e:?}"),
        Ok(_) => panic!("expected AmbiguousPort, but add_edge succeeded"),
    }
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
    async fn execute(&mut self, _inputs: &[NodeInput]) -> Result<NamedDataFrames, DagError> {
        Err(DagError::Schedule("kaboom".into()))
    }
}

#[tokio::test]
async fn failure_cascades() {
    let mut engine = DataEngine::builder().build();
    // Source-like boom node (no input ports) so it passes port validation.
    let boom_meta = NodeMeta::new("boom")
        .with_inputs(vec![])
        .with_outputs(vec![Port::default_port()]);
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
            // No input ports: standalone nodes pass port validation.
            let meta = NodeMeta::new(&id)
                .with_inputs(vec![])
                .with_outputs(vec![Port::default_port()]);
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
