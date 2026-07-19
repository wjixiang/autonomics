use std::sync::Arc;

use datafusion::{common::HashMap, prelude::SessionContext};
use datalake::Datalake;

use serde::Serialize;

use super::error::{Error, Result};

use crate::dag::DagNode;
use crate::nodes::{
    ldsc_hsq::LdscHsqNodeFactory, ldsc_rg::LdscRgNodeFactory,
    linear_regression::LinearRegressionNodeFactory, mock_node::MockNodeFactory, mr::MrNodeFactory,
    sink::SinkNodeFactory, source::SourceNodeFactory, sql_node::SqlNodeFactory,
};

pub trait NodeFactory: Send + Sync {
    fn kind(&self) -> &'static str;
    fn spec_schema(&self) -> schemars::Schema;
    fn build(&self, spec: serde_json::Value, node_ctx: NodeCtx) -> Result<Box<dyn DagNode>>;
}

/// Context dependencies for DagNodes
#[derive(Clone)]
pub struct NodeCtx {
    /// Datafusion SessionContext shared by whole data engine thread.
    ///
    /// TODO: Instead of passing whole SessionContext, it is better to provider ingrediants that
    /// consititude SessionContext, then let nodes to build their own ctx by need.
    pub session: SessionContext,
    pub datalake: Arc<Datalake>,
}

/// Summary of a registered node kind returned by [`NodeRegistry::list_nodes`].
#[derive(Debug, Clone, Serialize)]
pub struct NodeInfo {
    pub kind: String,
    pub schema: schemars::Schema,
}

/// The single source of truth of "which node kinds exist and how to build one from spec."
///
/// This object handles DagNode building and generalize operation of different nodes into uniformed
/// methods.
pub struct NodeRegistry {
    node_ctx: NodeCtx,
    nodes: HashMap<String, Box<dyn NodeFactory>>,
}

impl NodeRegistry {
    /// NOTE: currently all nodes are directly registered in this function. In future,
    pub fn new(ctx: SessionContext, datalake: Arc<Datalake>) -> Self {
        let node_ctx = NodeCtx {
            session: ctx,
            datalake,
        };

        let mut registry = Self {
            node_ctx,
            nodes: Default::default(),
        };
        registry.register(Box::new(SqlNodeFactory {}));
        registry.register(Box::new(SourceNodeFactory {}));
        registry.register(Box::new(SinkNodeFactory {}));
        registry.register(Box::new(LdscHsqNodeFactory {}));
        registry.register(Box::new(LdscRgNodeFactory {}));
        registry.register(Box::new(LinearRegressionNodeFactory {}));
        registry.register(Box::new(MockNodeFactory {}));
        registry.register(Box::new(MrNodeFactory {}));
        registry
    }

    pub fn register(&mut self, factory: Box<dyn NodeFactory>) {
        self.nodes.insert(factory.kind().to_string(), factory);
    }

    fn get_node_factory(&self, node_kind: &str) -> Result<&dyn NodeFactory> {
        self.nodes
            .get(node_kind)
            .map(|b| b.as_ref())
            .ok_or(Error::FactoryNotFound {
                kind: node_kind.to_string(),
            })
    }

    pub fn build_node(&self, node_kind: &str, spec: serde_json::Value) -> Result<Box<dyn DagNode>> {
        let node_factory = self.get_node_factory(node_kind)?;
        let node = node_factory.build(spec, self.node_ctx.clone())?;
        Ok(node)
    }

    pub fn get_node_spec(&self, node_kind: &str) -> Result<schemars::Schema> {
        Ok(self.get_node_factory(node_kind)?.spec_schema())
    }

    /// Return metadata of every registered node kind (kind + JSON Schema).
    pub fn list_nodes(&self) -> Vec<NodeInfo> {
        self.nodes
            .iter()
            .map(|(kind, factory)| NodeInfo {
                kind: kind.clone(),
                schema: factory.spec_schema(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal valid specs for each registered node kind — used by the
    /// invariant test to build a node without needing real files / data.
    fn fixture_spec(kind: &str) -> serde_json::Value {
        match kind {
            "sql" => serde_json::json!({"sql_query": "SELECT 1"}),
            "source" => serde_json::json!({"type": "file", "path": "/tmp/dummy.csv"}),
            "sink" => {
                serde_json::json!({"type": "file", "path": "/tmp/dummy_out.csv", "format": "csv"})
            }
            "linear_regression" => {
                serde_json::json!({"x_columns": ["x"], "y_column": "y"})
            }
            "ldsc" => serde_json::json!({"m": [1000000.0], "n_blocks": 200}),
            "ldsc_rg" => serde_json::json!({"m": [1000000.0], "n_blocks": 200}),
            "mr" => serde_json::json!({"action": 2, "method_list": ["mr_egger_regression"]}),
            "mock" => serde_json::json!({}),
            other => panic!("no fixture spec for kind '{other}'"),
        }
    }

    /// Invariant: every registered factory's `kind()` matches the
    /// `node_type()` of a node built by that factory. Under Option B
    /// this should always hold (factories delegate to `<N as DagNode>::kind()`),
    /// but the test catches any future drift.
    #[test]
    fn all_factories_kind_matches_node_kind() {
        let ctx = datafusion::prelude::SessionContext::new();
        let datalake = std::sync::Arc::new(datalake::Datalake::default());
        let registry = NodeRegistry::new(ctx, datalake);

        let nodes = registry.list_nodes();
        assert!(!nodes.is_empty(), "registry should have at least one kind");

        for info in &nodes {
            let kind = &info.kind;
            let spec = fixture_spec(kind);
            let node = registry
                .build_node(kind, spec)
                .unwrap_or_else(|e| panic!("build_node({kind}) failed: {e}"));
            assert_eq!(
                node.kind(),
                kind,
                "factory kind '{}' must match built node's kind()",
                kind
            );
        }
    }
}
