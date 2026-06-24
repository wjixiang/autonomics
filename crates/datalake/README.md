# Datalake

Unified connecting layer of Apache Iceberg, plus an in-memory analytical dataset
model for the agent analytics pipeline.

## Modules

- **`aether`** — `AetherWorkspace`: DataFusion `SessionContext` with the Iceberg
  catalog mounted; table discovery, schema inspection, SQL queries.
- **`datalake`** — `Datalake`: Iceberg REST catalog wrapper (S3-backed).
- **`dataset`** — `AetherDataset` + `DatasetStore`: in-memory analytical data
  model inspired by Spark RDD. See **[docs/dataset.md](docs/dataset.md)** for the
  full design.

## Documentation

- [AetherDataset 数据模型设计文档](docs/dataset.md) — core model, store,
  stat-primitives bridge, agent tools, and the L1–L5 tool roadmap.
