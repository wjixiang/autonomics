use async_trait::async_trait;
use datafusion::{common::HashMap, execution::SessionStateBuilder, prelude::SessionContext};
use thiserror::Error;

use super::meta::{DagNode, NodeInput, NodeMeta};

use crate::data_engine::dag::{DagError, graph::PortOutputs};

#[derive(Debug, Error)]
pub enum SqlNodeError {
    #[error("Invalid input: {message}")]
    InvalidInput { message: String },
    #[error("register upstream DataFrame as view failed")]
    RegisterView(#[source] datafusion::error::DataFusionError),
}

impl From<SqlNodeError> for DagError {
    fn from(e: SqlNodeError) -> Self {
        match e {
            SqlNodeError::RegisterView(source) => DagError::DataFusion(source),
            SqlNodeError::InvalidInput { message } => DagError::Schedule(message),
        }
    }
}

/// A transform node: registers each upstream input as a named table and runs a
/// SQL query over them. Single output port, variadic input.
///
/// Each upstream input arriving on port `N` is registered as the table
/// `port_{N}`, so the SQL references it as e.g. `FROM port_0` (or
/// `JOIN port_1` for a multi-input node). Input port count is not fixed
/// (`set_fixed_input(false)`); the ports are whatever the wiring connects.
#[derive(Clone)]
pub struct SqlNode {
    meta: NodeMeta,
    sql_query: String,
    ctx: SessionContext,
}

impl SqlNode {
    pub fn new(query: String, ctx: SessionContext) -> Self {
        let meta = NodeMeta::new()
            .add_output_port(None)
            .set_fixed_input(false);
        Self {
            meta,
            sql_query: query,
            ctx,
        }
    }

    /// Create a [`SqlNode`] from a pre-built [`NodeMeta`] (useful for
    /// multi-input join nodes that declare several input ports).
    pub fn from_meta(meta: NodeMeta, query: String, ctx: SessionContext) -> Self {
        Self {
            meta,
            sql_query: query,
            ctx,
        }
    }

    pub fn set_sql_query(&mut self, query: &str) {
        self.sql_query = query.to_string();
    }
}

#[async_trait]
impl DagNode for SqlNode {
    fn meta(&self) -> &NodeMeta {
        &self.meta
    }

    fn clone_box(&self) -> Box<dyn DagNode> {
        Box::new((*self).clone())
    }

    fn node_type(&self) -> &str {
        "sql"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn execute(&mut self, inputs: &[NodeInput]) -> Result<PortOutputs, DagError> {
        if inputs.is_empty() {
            return Err(SqlNodeError::InvalidInput {
                message: "SqlNode requires at least one upstream input".to_string(),
            }
            .into());
        }

        let state = SessionStateBuilder::new()
            .with_runtime_env(self.ctx.runtime_env())
            .with_catalog_list(self.ctx.state().catalog_list().clone())
            .with_default_features()
            .build();

        let ctx = SessionContext::new_with_state(state);
        for inp in inputs {
            // Register each upstream DataFrame under `port_{port}`. The fresh
            // context isolates the table namespace so concurrent SqlNodes never
            // collide on the same `port_N` slot, while sharing the engine's
            // `RuntimeEnv` keeps its object stores reachable.
            //
            // `DataFrame::into_view()` discards the DataFrame's own
            // `SessionState` and replans the scan against whichever context
            // consumes the view, so this context MUST carry the object store:
            // a bare `SessionContext::new()` registers only the default
            // `LocalFileSystem` under `file://`, so a CSV-backed upstream
            // ListingTable would find no file and silently return 0 rows.
            let view = inp.data.clone().into_view();
            ctx.register_table(format!("port_{}", inp.port), view)
                .map_err(SqlNodeError::RegisterView)?;
        }
        let out = ctx.sql(&self.sql_query).await?;
        let mut res: PortOutputs = HashMap::new();
        res.insert(0, out);
        Ok(res)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use arrow_array::{ArrayRef, Int32Array, RecordBatch, StringArray, StructArray};
    use arrow_schema::{DataType, Field, Fields, Schema};
    use datafusion::prelude::DataFrame;
    use std::sync::Arc;

    /// Create a [`SessionContext`] with a simple int32 column `x` (values `[1, 2, 3]`)
    /// registered as table `"src"`, and a [`SqlNode`] wired to the given `sql` query.
    fn setup_test_node(sql: &str) -> (SessionContext, SqlNode, DataFrame) {
        let ctx = SessionContext::new();
        let schema = Arc::new(Schema::new(vec![Field::new("x", DataType::Int32, false)]));
        let batch =
            RecordBatch::try_new(schema, vec![Arc::new(Int32Array::from(vec![1, 2, 3]))]).unwrap();
        let df = ctx.read_batch(batch).unwrap();
        ctx.register_table("src", df.clone().into_view()).unwrap();
        let node = SqlNode::new(sql.into(), ctx.clone());
        (ctx, node, df)
    }

    #[tokio::test]
    async fn test_smoke_register_table() {
        // Sanity check: a DataFrame can be turned into a TableProvider and
        // registered — this exercises the API path used by `execute`. The
        // DAG plumbing / schema wiring lives in higher-level integration
        // tests.
        let (ctx, node, _df) = setup_test_node("SELECT * FROM src WHERE x > 1");
        let result = ctx
            .sql(&node.sql_query)
            .await
            .unwrap()
            .collect()
            .await
            .unwrap();
        assert_eq!(result[0].num_rows(), 2);
    }

    #[tokio::test]
    async fn test_data_intake() {
        let (_ctx, mut node, df) = setup_test_node("SELECT * FROM port_0 WHERE x > 1");
        let input = NodeInput { port: 0, data: df };

        // Verify SQL node input mapping: it should use 'port_0' to reference data.
        let output = node.execute(&[input]).await.unwrap();
        dbg!(output);
    }

    /// Build a RecordBatch carrying a nested `Struct` column
    /// (`info { name: utf8, age: int32 }`) next to a scalar `id`, hand it
    /// to SqlNode, and run a SQL query that filters on one struct
    /// sub-field and projects another.
    ///
    /// Two companion tests cover the two halves of the end-to-end path:
    ///
    /// * `test_struct_df_construction` — locks down the data plumbing:
    ///   a RecordBatch with a Struct column becomes a DataFrame with
    ///   `info: Struct { name, age }`.
    /// * `test_struct_subfield_query` — runs a `WHERE info['age'] > 28`
    ///   / `SELECT info['name']` query through SqlNode and pins the
    ///   correct output. This is the regression test for the original
    ///   gap where SqlNode's per-execution SessionState didn't carry
    ///   DataFusion's default features, so the bracket-syntax lowering
    ///   (`RawFieldAccessExpr` → `get_field()`) and the `get_field` UDF
    ///   itself were unreachable. The fix is `with_default_features()`
    ///   on the builder in `SqlNode::execute`.
    fn build_struct_dataframe(ctx: &SessionContext) -> DataFrame {
        let info_fields = Fields::from(vec![
            Field::new("name", DataType::Utf8, false),
            Field::new("age", DataType::Int32, false),
        ]);
        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int32, false),
            Field::new("info", DataType::Struct(info_fields.clone()), false),
        ]));

        let name_array: ArrayRef = Arc::new(StringArray::from(vec!["Alice", "Bob", "Charlie"]));
        let age_array: ArrayRef = Arc::new(Int32Array::from(vec![30, 25, 35]));
        let info_array =
            StructArray::try_new(info_fields, vec![name_array, age_array], None).unwrap();

        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(Int32Array::from(vec![1, 2, 3])),
                Arc::new(info_array),
            ],
        )
        .unwrap();

        ctx.read_batch(batch).unwrap()
    }

    #[tokio::test]
    async fn test_struct_df_construction() {
        let ctx = SessionContext::new();
        let df = build_struct_dataframe(&ctx);
        let schema = df.schema();

        assert_eq!(schema.fields().len(), 2);
        let info_field = schema
            .field_with_name(None, "info")
            .expect("info column must exist");
        assert!(
            matches!(info_field.data_type(), DataType::Struct(_)),
            "info must be a Struct column, got {:?}",
            info_field.data_type(),
        );
        let DataType::Struct(info_fields) = info_field.data_type() else {
            unreachable!("asserted above");
        };
        assert_eq!(
            info_fields
                .iter()
                .map(|f| f.name().as_str())
                .collect::<Vec<_>>(),
            vec!["name", "age"],
        );
    }

    #[tokio::test]
    async fn test_struct_subfield_query() {
        let ctx = SessionContext::new();
        let df = build_struct_dataframe(&ctx);

        // Filter on `info['age']` and project `info['name']`. Bracket
        // syntax parses to `RawFieldAccessExpr`, which `FieldAccessPlanner`
        // lowers to `get_field(...)`; both live behind DataFusion's
        // `with_default_features()`, which `SqlNode::execute` must
        // install into its per-execution SessionState — see the
        // `.with_default_features()` call on the builder there.
        let sql = "SELECT id, info.name AS name \
                   FROM port_0 \
                   WHERE info['age'] > 28 \
                   ORDER BY id";
        let mut node = SqlNode::new(sql.into(), ctx.clone());
        let input = NodeInput { port: 0, data: df };
        let outputs = node.execute(&[input]).await.unwrap();
        let batches = outputs.get(&0).unwrap().clone().collect().await.unwrap();

        assert_eq!(batches.len(), 1, "expected a single RecordBatch");
        let rb = &batches[0];
        assert_eq!(rb.num_rows(), 2, "expected 2 rows where info.age > 28");

        let ids = rb.column(0).as_any().downcast_ref::<Int32Array>().unwrap();
        assert_eq!(ids.values(), &[1, 3]);

        let names = rb.column(1).as_any().downcast_ref::<StringArray>().unwrap();
        assert_eq!(names.value(0), "Alice");
        assert_eq!(names.value(1), "Charlie");
    }
}
