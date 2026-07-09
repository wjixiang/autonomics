use async_trait::async_trait;
use datafusion::{common::HashMap, prelude::SessionContext};
use thiserror::Error;

use super::meta::{DagNode, NodeInput, NodeMeta, Port};

use crate::data_engine::dag::{DagError, graph::NamedDataFrames};

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
/// SQL query over them. Single output port.
///
/// Input ports are taken from the supplied [`NodeMeta`] (default: one port
/// named `"default"`). To build a multi-input join, construct the meta with
/// several input ports, e.g.:
/// ```ignore
/// let meta = NodeMeta::new("join")
///     .with_inputs(vec![Port::new("left"), Port::new("right")]);
/// SqlNode::new(meta, "SELECT ... FROM {node}__left JOIN {node}__right ...".into(), ctx, "result".into())
/// ```
/// Each input is registered in the shared `SessionContext` under its
/// globally-unique `df_name` (`"{node_id}__{port}"`), which the SQL references.
#[derive(Clone)]
pub struct SqlNode {
    meta: NodeMeta,
    sql_query: String,
    ctx: SessionContext,
    output_df_name: String,
}

impl SqlNode {
    pub fn new(
        meta: NodeMeta,
        query: String,
        ctx: SessionContext,
        output_df_name: String,
    ) -> Self {
        // Output port is named after the output DataFrame. Input ports are
        // whatever the caller declared on the meta (default single "default").
        let meta = meta.with_outputs(vec![Port::new(output_df_name.clone())]);
        Self {
            meta,
            sql_query: query,
            ctx,
            output_df_name,
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

    async fn execute(&mut self, inputs: &[NodeInput]) -> Result<NamedDataFrames, DagError> {
        if inputs.is_empty() {
            return Err(SqlNodeError::InvalidInput {
                message: "SqlNode requires at least one upstream input".to_string(),
            }
            .into());
        }

        let ctx = self.ctx.clone();
        for inp in inputs {
            // register_table errors if the name already exists, so deregister
            // first.
            //
            // NOTE: SqlNodes share the engine's SessionContext, so a port
            // name is a single shared slot — give each upstream a distinct port
            // name when they carry different data.
            let _ = ctx.deregister_table(&inp.df_name);
            let view = inp.data.clone().into_view();
            ctx.register_table(&inp.df_name, view)
                .map_err(SqlNodeError::RegisterView)?;
        }
        let out = ctx.sql(&self.sql_query).await?;
        let mut res: NamedDataFrames = HashMap::new();
        res.insert(self.output_df_name.clone(), out);
        Ok(res)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use arrow_array::{Int32Array, RecordBatch};
    use arrow_schema::{DataType, Field, Schema};
    use std::sync::Arc;

    use super::super::meta::NodeMeta;

    #[tokio::test]
    async fn test_smoke_register_table() {
        // Sanity check: a DataFrame can be turned into a TableProvider and
        // registered — this exercises the API path used by `execute`. The
        // DAG plumbing / schema wiring lives in higher-level integration
        // tests.
        let ctx = SessionContext::new();
        let schema = Arc::new(Schema::new(vec![Field::new("x", DataType::Int32, false)]));
        let batch =
            RecordBatch::try_new(schema, vec![Arc::new(Int32Array::from(vec![1, 2, 3]))]).unwrap();
        let df = ctx.read_batch(batch).unwrap();
        let node = SqlNode::new(
            NodeMeta::new("sql"),
            "SELECT * FROM src WHERE x > 1".into(),
            ctx.clone(),
            "out".into(),
        );
        let view = df.into_view();
        ctx.register_table("src", view).unwrap();
        let result = ctx
            .sql(&node.sql_query)
            .await
            .unwrap()
            .collect()
            .await
            .unwrap();
        assert_eq!(result[0].num_rows(), 2);
        let _ = node;
    }
}
