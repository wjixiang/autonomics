use async_trait::async_trait;
use datafusion::prelude::{DataFrame, SessionContext};
use thiserror::Error;

use super::meta::{DagNode, NodeInput, NodeMeta};
use std::sync::Arc;

use crate::data_engine::dag::DagError;

#[derive(Debug, Error)]
pub enum SqlNodeError {
    #[error("Invalid input: {message}")]
    InvalidInput { message: String },
    #[error("register upstream DataFrame as view failed")]
    RegisterView(#[source] datafusion::error::DataFusionError),
}

impl SqlNodeError {
    fn kind(&self) -> &'static str {
        match self {
            SqlNodeError::InvalidInput { .. } => "sql.invalid_input",
            SqlNodeError::RegisterView(_) => "sql.register_view",
        }
    }
}

impl From<SqlNodeError> for DagError {
    fn from(value: SqlNodeError) -> Self {
        Self::ExecutionError {
            kind: value.kind(),
            source: Box::new(value),
        }
    }
}

/// A transform node: registers each upstream input as a named table/view
/// (the edge `port`) and runs a SQL query over them. Single output.
#[derive(Clone)]
pub struct SqlNode {
    meta: NodeMeta,
    sql_query: String,
    ctx: Arc<SessionContext>,
}

impl SqlNode {
    pub fn new(meta: NodeMeta, query: String, ctx: Arc<SessionContext>) -> Self {
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

    async fn execute(&mut self, inputs: &[NodeInput]) -> Result<Vec<DataFrame>, DagError> {
        if inputs.is_empty() {
            return Err(SqlNodeError::InvalidInput {
                message: "SqlNode requires at least one upstream input".to_string(),
            }
            .into());
        }

        let ctx = self.ctx.clone();
        for inp in inputs {
            // register_table errors if the name already exists, so deregister
            // first. NOTE: SqlNodes share the engine's SessionContext, so a port
            // name is a single shared slot — give each upstream a distinct port
            // name when they carry different data.
            let _ = ctx.deregister_table(&inp.port);
            let view = inp.data.clone().into_view();
            ctx.register_table(&inp.port, view)
                .map_err(SqlNodeError::RegisterView)?;
        }
        let out = ctx.sql(&self.sql_query).await?;
        Ok(vec![out])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use arrow_array::{Int32Array, RecordBatch};
    use arrow_schema::{DataType, Field, Schema};
    use datafusion::prelude::SessionContext;

    use super::super::meta::NodeMeta;

    #[tokio::test]
    async fn test_smoke_register_table() {
        // Sanity check: a DataFrame can be turned into a TableProvider and
        // registered — this exercises the API path used by `execute`. The
        // DAG plumbing / schema wiring lives in higher-level integration
        // tests.
        let ctx: Arc<SessionContext> = Arc::new(SessionContext::new());
        let schema = Arc::new(Schema::new(vec![Field::new("x", DataType::Int32, false)]));
        let batch =
            RecordBatch::try_new(schema, vec![Arc::new(Int32Array::from(vec![1, 2, 3]))]).unwrap();
        let df = ctx.read_batch(batch).unwrap();
        let node = SqlNode::new(
            NodeMeta::new("sql"),
            "SELECT * FROM src WHERE x > 1".into(),
            ctx.clone(),
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
