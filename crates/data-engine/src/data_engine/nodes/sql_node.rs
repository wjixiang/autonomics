use async_trait::async_trait;
use datafusion::prelude::DataFrame;
use thiserror::Error;

use crate::data_engine::dag::{DagError, DagNode, NodeMeta};

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

pub struct SqlNode {
    meta: NodeMeta,
    sql_query: String,
}

impl SqlNode {
    pub fn new(meta: NodeMeta, query: String) -> Self {
        Self {
            meta,
            sql_query: query,
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

    async fn execute(&mut self, inputs: &[DataFrame]) -> Result<Vec<DataFrame>, DagError> {
        let df = inputs.first().ok_or(SqlNodeError::InvalidInput {
            message: "Input is empty".to_string(),
        })?;

        let view = df.clone().into_view();
        let ctx = self.meta.ctx();
        ctx.register_table("src", view)?;
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

    use crate::data_engine::dag::NodeMeta;

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
            NodeMeta::new(
                "sql".into(),
                "sql_node".into(),
                Default::default(),
                ctx.clone(),
            ),
            "SELECT * FROM src WHERE x > 1".into(),
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
