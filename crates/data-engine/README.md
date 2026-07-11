# data-engine

`data-engine` is the DataFusion-based workflow engine used by autonomics. It models a data-analysis pipeline as a typed, directed acyclic graph (DAG), validates port connections, schedules ready nodes asynchronously, and retains each node's output for downstream consumers or inspection.

It is intentionally independent of the Agent loop. `data-engine-tools` adapts its channel-based runtime into Agentik `ToolFunction`s.

## Data flow

```text
SourceNode ── DataFrame ──> SqlNode / LinearRegressionNode ──> SinkNode
       │                           │
       └──────────── fan-out ──────┴──> more transformations
```

Every edge connects one named output port to one named input port. The public convenience APIs use port `0` for the common single-input/single-output case. A graph must be acyclic and have compatible ports before it runs.

## Built-in nodes

| Node | Inputs → outputs | Purpose |
| --- | --- | --- |
| `SourceNode` | 0 → 1 | Reads CSV, Parquet, an Iceberg table, or a biological file into a DataFusion `DataFrame`. |
| `SqlNode` | 1+ → 1 | Runs a DataFusion SQL query. Inputs are registered in an isolated context as `port_0`, `port_1`, and so on. |
| `LinearRegressionNode` | 1 → 1 | Fits an OLS regression with configurable predictor columns and optional intercept. |
| `SinkNode` | 1 → 0 | Writes CSV or Parquet. Iceberg output is represented in the API but is not implemented yet. |
| `CacheSourceNode` | 0 → 1 | Restores a cached output through the Iceberg-backed cache integration. |

`biofusion` supplies the biological readers used by `SourceNode`: VCF, BCF, FASTA, FASTQ, BED, GTF, GFF, SAM, BAM, CRAM, BigWig, and BigBed. Formats are normally inferred from the file suffix, including compressed suffixes such as `.vcf.gz`.

## Build and run a pipeline

```rust,no_run
use data_engine::{DataEngine, Sink, Source, WriteFormat};

# async fn example() -> Result<(), Box<dyn std::error::Error>> {
let mut engine = DataEngine::builder().build();

engine
    .source_node(
        "variants",
        Source::File { path: "input.vcf.gz".into(), format: None },
        "variants",
    )?
    .sql_node("filtered", "SELECT * FROM port_0", "filtered")?
    .sink_node(
        "write",
        Sink::File { path: "output.parquet".into(), format: WriteFormat::Parquet },
    )?
    .add_edge("variants", "filtered", 0, 0)?
    .add_edge("filtered", "write", 0, 0)?;

let report = engine.run().await?;
assert!(report.ok, "pipeline errors: {:?}", report.errors);
# Ok(())
# }
```

Use `engine.view_dag()` to obtain a Graphviz DOT representation. `engine.get_output(node_id).await` returns the in-memory port outputs of a completed node.

## Iceberg and object storage

`DataEngine::builder()` creates a standalone DataFusion session. Add integrations only when the pipeline needs them:

```rust,no_run
use std::sync::Arc;
use data_engine::DataEngine;
use fs::OpendalFileStorage;

# async fn example() -> Result<(), Box<dyn std::error::Error>> {
let files = Arc::new(OpendalFileStorage::new("/data"));
let engine = DataEngine::builder()
    .register_opendal_fs(files)?
    .register_iceberg()
    .await?
    .build();
# Ok(())
# }
```

The engine uses a REST Iceberg catalog when registered. Reading an Iceberg source is supported; writing an Iceberg sink currently returns a clear `not yet implemented` error.

## Agent-facing runtime

`runtime::spawn_with_engine` hosts an `IcebergDataEngine` in a Tokio task and returns a cloneable `DataEngineClient`. Requests use an unbounded command channel plus one-shot replies. `data-engine-tools` exposes the following operations to an Agent:

- add source, SQL, sink, and linear-regression nodes;
- connect nodes with default or explicit ports;
- run, view, clear, and remove DAG nodes; and
- retrieve paginated output.

This boundary serializes graph mutations while allowing the Agent runtime and UI to remain independent of the engine implementation.

## Tests

Run the crate tests with a writable target directory:

```bash
CARGO_TARGET_DIR=/tmp/autonomics-target cargo test -p data-engine
```

The crate includes fixture-driven pipeline tests, graph validation tests, scheduler/concurrency tests, and an ignored Iceberg connectivity test. The latter requires a reachable configured catalog.
