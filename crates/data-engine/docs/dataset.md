# AetherDataset 数据模型设计文档

> Agent 数据分析流水线的核心内存数据抽象，对标 Spark RDD/Dataset。

## 1. 背景与动机

Agent 系统需要分析中间态的数据对象来执行数据分析任务。原始设计存在三个断层：

1. **数据流没有结构化抽象** —— Iceberg 表查询结果直接以 `Vec<RecordBatch>` 或 pretty-printed 字符串返回，Agent 无法稳定地引用、变换、组合这些中间结果。
2. **统计库与数据层脱节** —— `stat-primitives` 的回归/meta-analysis 函数完全基于 `&[f64]` 切片，与 Arrow 列式类型系统没有任何桥接。
3. **缺少命名工作内存** —— Agent 在多步分析中产生的中间数据集无处寄存，无法跨工具调用复用。

`AetherDataset` + `DatasetStore` 解决上述问题，构成「持久化数据湖 → 内存分析模型 → 统计计算」的完整链路。

## 2. 整体架构

```
┌─────────────────────────────────────────────────────────┐
│  Agent Tools (iceberg-tools/src/dataset/)                │
│  L1: dataset_load_table                                  │
│  L2: list / describe / preview / drop                     │
│  L3: select / sort / limit / union / sql                 │
│  L4: summarize / ols / ivw / egger                       │
└────────────────────────┬────────────────────────────────┘
                         │ 调用
┌────────────────────────▼────────────────────────────────┐
│  DatasetStore   (datalake/src/dataset/store.rs)          │
│  · 命名注册表  HashMap<String, Arc<AetherDataset>>       │
│  · 共享 SessionContext（含 iceberg catalog）              │
│  · read_table / sql_to_dataset / sql_query               │
└────────────────────────┬────────────────────────────────┘
                         │ 持有
┌────────────────────────▼────────────────────────────────┐
│  AetherDataset   (datalake/src/dataset/mod.rs)           │
│  · name + schema(SchemaRef) + batches(Vec<RecordBatch>)  │
│  · provenance 血缘追踪                                    │
│  · 变换(select/filter/sort/union/...) 返回新实例           │
│  · 物化(collect/pretty_format)                            │
│  · 列提取(extract_f64/f32/string) → stat-primitives 桥接  │
└────────────────────────┬────────────────────────────────┘
                         │ 喂数据
┌────────────────────────▼────────────────────────────────┐
│  stat-primitives   (零依赖)                               │
│  ols / wls / ivw / mr_egger / 描述统计 / 分布              │
└──────────────────────────────────────────────────────────┘
```

## 3. AetherDataset —— 对标 Spark RDD

### 设计原则映射

| RDD 属性 | AetherDataset 等价实现 |
|---|---|
| **不可变 (immutable)** | 每次变换返回新实例，原实例不变 |
| **分区 (partitioned)** | `Vec<RecordBatch>` 即逻辑分区，预留 `rayon` 并行 |
| **类型化 (typed)** | Arrow `SchemaRef` 强制列类型与可空性 |
| **变换 (transformations)** | `select` / `filter_by` / `sort_by` / `union` / `limit` |
| **动作 (actions)** | `collect` / `row_count` / `extract_f64` / `pretty_format` |
| **血缘 (lineage)** | `Provenance` 枚举追踪完整推导链 |

### 数据布局

```
AetherDataset {
    name:       "gwas_significant"
    schema:     [snp: Utf8, beta: Float64, se: Float64, p_value: Float64]
    batches:    [RecordBatch(0..999), RecordBatch(1000..1999), ...]   ← 分区
    provenance: Transform { op: "filter", parents: ["gwas_raw"] }     ← 血缘
}
```

### API 分类

#### 构造
- `new(name, batches)` — 推断 schema
- `with_schema(name, schema, batches)` — 显式 schema
- `empty(name, schema)` — 空数据集
- `with_provenance(prov)` — 设置血缘

#### 元数据访问
`name()` · `schema()` · `row_count()` · `column_count()` · `num_partitions()` ·
`batches()` · `provenance()` · `column_names()` · `field(name)` ·
`has_column(name)` · `column_index(name)` · `is_empty()` · `schema_json()`

#### 列提取（→ stat-primitives 桥接）
- `extract_f64(col, null_policy)` — 数值列 → `Vec<f64>`
- `extract_f32(col)` — Float32 列 → `Vec<f32>`
- `extract_string(col)` — 字符串列 → `Vec<Option<String>>`
- `extract_f64_columns(&[cols], policy)` — 多列 → `Vec<Vec<f64>>`（供 `ols()` 用）

#### 变换（返回新实例）
`select(&[cols])` · `drop_columns(&[cols])` · `rename_column(old, new)` ·
`limit(n)` · `take(n)` · `union(other)` · `sort_by(&[(col, asc)])` ·
`filter_by(closure)` · `map_column(col, output, f)`

#### 物化
`collect()` · `pretty_format()` · `pretty_head(n)`

### NullPolicy（空值策略）

`extract_f64` 提取数值列时的空值处理协议：

```rust
pub enum NullPolicy {
    DropNulls,     // 跳过 null（默认）
    Fill(f64),     // 用指定值填充
    Reject,        // 遇 null 报错
}
```

因为 `stat-primitives` 只接受 `&[f64]`，必须明确 null 的去向。

### Provenance（血缘）

```rust
pub enum Provenance {
    Query { sql: String },                       // SQL 查询产生
    Table { table: String },                     // Iceberg 表加载
    Transform { op: String, parents: Vec<String> }, // 对父数据集的变换
    Manual,                                       // 手工构造
}
```

每次变换自动记录 `op` 与 `parents`，构成可追溯的数据血缘链。

## 4. DatasetStore —— Agent 工作内存

命名注册表，持有共享 `SessionContext`（含已注册的 `iceberg` catalog）。

### 注册表 CRUD
`new(ctx)` · `from_workspace(&ws)` · `put(ds)` · `put_overwrite(ds)` ·
`get(name)` · `drop(name)` · `list()` · `exists(name)` · `ctx()`

### 摄入（L1）
- `read_table(name, namespace, table, columns?, filter?, limit?)` — **Iceberg 表 → 注册数据集**
- `peek_table(namespace, table, columns?, filter?, limit?)` — **Iceberg 表 → 临时数据集（不注册）**
- `sql_to_dataset(name, sql)` — SQL → 注册数据集
- `sql_query(sql)` — SQL → 临时数据集（不入注册表）

### 变换（L3）
- `map_expr(name, source, column, expr, output_col)` — DataFusion 表达式变换列（可替换或新增列）

### 内部 helper
- `build_table_sql(namespace, table, columns, filter, limit)` — 构造 `SELECT … FROM iceberg."ns"."table"`（`read_table`/`peek_table` 共用；必须用点号 3 段名 `catalog.schema.table`，逗号形式会被 SQL 当成 cross join）
- `collect_df(name, df, provenance)` — DataFrame → AetherDataset（共用）
- `insert_dataset(ds)` — 入注册表（共用，仅 `read_table`/`sql_to_dataset` 调用）
- `register_all_as_tables()` / `register_as_table(ds)` — 注册为 DataFusion MemTable

### SQL 变换机制
`sql_to_dataset` 执行前自动把所有已注册数据集注册为 DataFusion `MemTable`，因此 SQL 可跨数据集引用：

```sql
SELECT a.snp, a.beta, b.freq
FROM   gwas_sig a JOIN allele_freq b ON a.snp = b.snp
```

## 5. Agent 工具层（iceberg-tools/src/dataset/）

每个工具遵循 `agentik-core` 的 `ToolFunction` 模式：`#[derive(ToolInput)]` 输入结构 + 持有 `Arc<DatasetStore>` 的工具结构。

### 已实现工具

**Dataset 工具**（`iceberg-tools/src/dataset/`，操作内存数据集）:

| 工具 | 封装 | 对 store | 状态 |
|---|---|---|---|
| `dataset_load_table` | `read_table` | 注册新数据集（L1） | ✅ |
| `dataset_list` | `list` | 只读（L2） | ✅ |
| `dataset_describe` | `get` + `schema_json` | 只读（L2） | ✅ |
| `dataset_preview` | `get` + `pretty_head` | 只读（L2） | ✅ |
| `dataset_drop` | `drop` | 删除（L2） | ✅ |
| `dataset_select` | `select` | 变换，in-place 替换（L3） | ✅ |
| `dataset_sort` | `sort_by` | 变换，in-place 替换（L3） | ✅ |
| `dataset_limit` | `limit` | 变换，in-place 替换（L3） | ✅ |
| `dataset_union` | `union` | 变换，in-place 替换（L3） | ✅ |
| `dataset_sql` | `sql_to_dataset` | SQL 变换（L3） | ✅ |
| `dataset_map` | `map_expr` | 表达式变换（L3） | ✅ |
| `dataset_summarize` | `extract_f64` + descriptive | 分析，JSON 结果（L4） | ✅ |
| `dataset_ols` | `extract_f64_columns` + ols | 分析，JSON 结果（L4） | ✅ |
| `dataset_ivw` | `extract_f64` × 3 + ivw | 分析，JSON 结果（L4） | ✅ |
| `dataset_egger` | `extract_f64` × 3 + mr_egger | 分析，JSON 结果（L4） | ✅ |

**Iceberg 工具**（`iceberg-tools/src/`，操作 Iceberg catalog，前缀 `iceberg_`）:

| 工具 | 对 store | 状态 |
|---|---|---|---|
| `iceberg_preview_table` | 不物化，返回 schema + rows | ✅ |
| `iceberg_list_namespaces` | — | ✅ |
| `iceberg_create_namespace` | — | ✅ |
| `iceberg_namespace_exists` | — | ✅ |
| `iceberg_drop_namespace` | — | ✅ |
| `iceberg_list_tables` | — | ✅ |
| `iceberg_list_tables_in_namespace` | — | ✅ |
| `iceberg_table_exists` | — | ✅ |
| `iceberg_describe_table` | — | ✅ |
| `iceberg_load_table` | — | ✅ |
| `iceberg_create_table` | — | ✅ |
| `iceberg_drop_table` | — | ✅ |
| `iceberg_rename_table` | — | ✅ |

`iceberg_preview_table` 与 `dataset_preview` 的区别：前者直接看 **Iceberg 表**（不物化、不注册），后者看**已注册的内存数据集**。

### 工具层路线图（L1–L5）

| 层 | 工具原语 | 状态 |
|---|---|---|
| **L1 摄入** | `dataset_load_table` | ✅ |
| **L2 检视** | list / describe / preview / drop | ✅ |
| **L3 变换** | select / sort / limit / union / sql / map | ✅ |
| **L4 分析** | summarize / ols / ivw / egger | ✅ |
| **L5 输出** | export / to_json | ⏳ 待封装 |

> L3 中简单变换（select/filter/limit/union）走 `AetherDataset` 自带方法；复杂变换（join/group_by/sort/distinct）走 `DatasetStore` 的 SQL 能力。

## 6. 典型用法

### 从 Iceberg 表加载并过滤
```rust
let ds = store.read_table(
    "gwas_sig", "analytics", "gwas_summary",
    Some(&["snp".into(), "beta".into(), "se".into(), "p_value".into()]),
    Some("p_value < 5e-8"),
    None,
).await?;
```

### 提取数值列做回归
```rust
use stat_primitives::regression::ols;

let beta = ds.extract_f64("beta", NullPolicy::Reject)?;
let se   = ds.extract_f64("se", NullPolicy::Reject)?;
let y    = ds.extract_f64("outcome", NullPolicy::Reject)?;
let result = ols(&[&beta, &se], &y, true)?;
```

### Agent 调用工具
```
Agent: dataset_load_table(name="gwas_sig", namespace="analytics",
                          table="gwas_summary", filter="p_value < 5e-8")
→ { "dataset": "gwas_sig", "row_count": 1234, "column_count": 4, "schema": [...] }
```

## 7. 运行时接线

`DatasetStore` 在 agent 启动时构造，共享 `AetherWorkspace` 的 `SessionContext`：

```rust
// apps/phloem-tui/src/agent_runtime.rs
let workspace = Arc::new(AetherWorkspace::new().await?);
let store = Arc::new(DatasetStore::from_workspace(&workspace));
let mut tools = iceberg_tools::iceberg_registrations(workspace);
tools.extend(iceberg_tools::dataset_registrations(store));
```

`phloem-server` 同样接线（见 `services/agent_manager.rs`）。

## 8. 设计决策记录

1. **Eager 物化而非 lazy** —— Agent 场景需要立即可查行数/预览，DataFusion 的 lazy DataFrame 适合内部执行，对外暴露 eager 语义。
2. **复用 Arrow SchemaRef** —— 不重新定义 schema 类型，与 DataFusion/Arrow 生态一致。
3. **变换走 SQL，简单操作走原生方法** —— join/group_by 等复杂关系代数交给 DataFusion 优化器；select/filter 等直接用 Arrow compute，避免不必要的 SQL 开销。
4. **数据源仅 Iceberg** —— 不引入 S3/本地文件读取，所有持久化数据统一经 Iceberg catalog。
5. **stat-primitives 保持零依赖** —— 桥接通过 `extract_f64` 单向完成，不污染统计库。

## 9. 文件清单

| 文件 | 内容 |
|---|---|
| `crates/datalake/src/dataset/mod.rs` | `AetherDataset` 核心模型 |
| `crates/datalake/src/dataset/store.rs` | `DatasetStore` 注册表 + `read_table`/`peek_table` reader |
| `crates/datalake/src/error.rs` | `DatasetError` |
| `crates/iceberg-tools/src/dataset/mod.rs` | 工具模块 + `dataset_registrations`（L1–L4 全部 15 个工具）|
| `crates/iceberg-tools/src/dataset/load_table.rs` | `dataset_load_table`（L1）|
| `crates/iceberg-tools/src/dataset/list.rs` | `dataset_list`（L2）|
| `crates/iceberg-tools/src/dataset/describe.rs` | `dataset_describe`（L2）|
| `crates/iceberg-tools/src/dataset/preview.rs` | `dataset_preview`（L2）|
| `crates/iceberg-tools/src/dataset/drop.rs` | `dataset_drop`（L2）|
| `crates/iceberg-tools/src/dataset/select.rs` | `dataset_select`（L3）|
| `crates/iceberg-tools/src/dataset/sort.rs` | `dataset_sort`（L3）|
| `crates/iceberg-tools/src/dataset/limit.rs` | `dataset_limit`（L3）|
| `crates/iceberg-tools/src/dataset/union.rs` | `dataset_union`（L3）|
| `crates/iceberg-tools/src/dataset/sql.rs` | `dataset_sql`（L3）|
| `crates/iceberg-tools/src/dataset/map.rs` | `dataset_map`（L3）|
| `crates/iceberg-tools/src/dataset/summarize.rs` | `dataset_summarize`（L4）|
| `crates/iceberg-tools/src/dataset/ols.rs` | `dataset_ols`（L4）|
| `crates/iceberg-tools/src/dataset/ivw.rs` | `dataset_ivw`（L4）|
| `crates/iceberg-tools/src/dataset/egger.rs` | `dataset_egger`（L4）|

## 10. 测试

- `AetherDataset` 单测：构造、列提取、空值策略、变换、血缘（25 个）
- `DatasetStore` 单测：CRUD、SQL 变换、跨数据集查询（8 个）
- 当前共 **54 个测试全绿**（33 AetherDataset + 11 DatasetStore + 6 iceberg-tools common + 4 integration）
