use std::sync::Arc;

use datafusion::{
    catalog::CatalogProvider,
    common::HashMap,
    execution::runtime_env::RuntimeEnv,
    prelude::{SessionConfig, SessionContext},
};
use datalake::Datalake;

use serde::Serialize;

use super::error::{Error, Result};
use crate::dag::DagNode;
use crate::nodes::meta::NodePorts;
use crate::nodes::{
    echo_node::EchoNodeFactory, ldsc_hsq::LdscHsqNodeFactory, ldsc_rg::LdscRgNodeFactory,
    linear_regression::LinearRegressionNodeFactory, mr::MrNodeFactory,
    sink::SinkNodeFactory, source::SourceNodeFactory, sql_node::SqlNodeFactory,
    test_source::TestSourceFactory,
};

/// Build a fresh, isolated [`SessionContext`].
///
/// Each call creates a **new** `CatalogList` (so `register_table("port_0", ...)`
/// never collides with another node's registration), while sharing the
/// engine-wide [`RuntimeEnv`] so object stores remain reachable.
///
/// If an `iceberg_catalog` is provided, it is registered under `"iceberg"`
/// on the fresh context.
pub fn new_isolated_ctx(
    runtime_env: Arc<RuntimeEnv>,
    iceberg_catalog: Option<Arc<dyn CatalogProvider>>,
) -> SessionContext {
    let ctx = SessionContext::new_with_config_rt(SessionConfig::new(), runtime_env);
    if let Some(cat) = iceberg_catalog {
        ctx.register_catalog("iceberg", cat);
    }
    ctx
}

pub trait NodeFactory: Send + Sync {
    fn kind(&self) -> &'static str;
    fn spec_schema(&self) -> schemars::Schema;
    /// The static port layout for this node kind — the input/output ports
    /// every instance of this kind will declare. Queryable without
    /// instantiating a node (mirrors [`NodeFactory::spec_schema`]).
    fn ports(&self) -> NodePorts;
    fn build(&self, spec: serde_json::Value, node_ctx: NodeCtx) -> Result<Box<dyn DagNode>>;
}

/// Ingredients for building an isolated [`SessionContext`] per node execution.
///
/// Instead of sharing a single `SessionContext` (which causes CatalogList
/// collisions on `register_table`), nodes receive the `RuntimeEnv` and an
/// optional `Iceberg` catalog, and construct their own context at execution
/// time via [`new_isolated_ctx`].
#[derive(Clone)]
pub struct NodeCtx {
    /// Shared object-store registry — all nodes reference the same
    /// `RuntimeEnv` so file:// / s3:// stores registered by the engine
    /// builder are reachable.
    pub runtime_env: Arc<RuntimeEnv>,
    /// Optional Iceberg `CatalogProvider`. Nodes that need to query
    /// `iceberg.*` tables receive `Some`; others receive `None`.
    pub iceberg_catalog: Option<Arc<dyn CatalogProvider>>,
    /// Iceberg REST catalog handle, used by LDSC nodes for table-level
    /// operations (create/drop/load) that go through the Iceberg API
    /// directly rather than DataFusion SQL.
    pub datalake: Arc<Datalake>,
}

/// Summary of a registered node kind returned by [`NodeRegistry::list_nodes`].
#[derive(Debug, Clone, Serialize)]
pub struct NodeInfo {
    pub kind: String,
    pub schema: schemars::Schema,
    /// Static input/output port layout for this kind.
    pub ports: NodePorts,
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
    /// NOTE: currently all nodes are directly registered in this function.
    pub fn new(
        runtime_env: Arc<RuntimeEnv>,
        iceberg_catalog: Option<Arc<dyn CatalogProvider>>,
        datalake: Arc<Datalake>,
    ) -> Self {
        let node_ctx = NodeCtx {
            runtime_env,
            iceberg_catalog,
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
        registry.register(Box::new(EchoNodeFactory {}));
        registry.register(Box::new(TestSourceFactory {}));
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

    /// Return metadata of every registered node kind (kind + JSON Schema + ports).
    pub fn list_nodes(&self) -> Vec<NodeInfo> {
        self.nodes
            .iter()
            .map(|(kind, factory)| NodeInfo {
                kind: kind.clone(),
                schema: factory.spec_schema(),
                ports: factory.ports(),
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
            "echo" => serde_json::json!({}),
            "test_source" => serde_json::json!({"dataset": "iris"}),
            other => panic!("no fixture spec for kind '{other}'"),
        }
    }

    /// Helper: build a `NodeRegistry` with a bare (no iceberg) context for tests.
    fn test_registry() -> NodeRegistry {
        let ctx = SessionContext::new();
        let runtime_env = ctx.runtime_env();
        NodeRegistry::new(runtime_env, None, Arc::new(Datalake::default()))
    }

    /// Invariant: every registered factory's `kind()` matches the
    /// `node_type()` of a node built by that factory. Under Option B
    /// this should always hold (factories delegate to `<N as DagNode>::kind()`),
    /// but the test catches any future drift.
    #[test]
    fn all_factories_kind_matches_node_kind() {
        let registry = test_registry();

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

            // The factory's externally-queryable port layout must match the
            // layout the built node actually declares — otherwise the agent
            // (and `add_edge` validation) would be lied to.
            let factory_ports = registry.get_node_factory(kind).unwrap().ports();
            let node_ports = node.ports();
            assert_eq!(
                factory_ports.input_ports().len(),
                node_ports.input_ports().len(),
                "kind '{kind}': factory.ports() input count must match built node's"
            );
            assert_eq!(
                factory_ports.output_ports().len(),
                node_ports.output_ports().len(),
                "kind '{kind}': factory.ports() output count must match built node's"
            );
            assert_eq!(
                factory_ports.is_fixed_input(),
                node_ports.is_fixed_input(),
                "kind '{kind}': factory.ports() input-fixed flag must match built node's"
            );
        }
    }

    /// `NodeInfo` (returned by `list_nodes`) must serialize, and each entry's
    /// `ports` field must round-trip into a JSON object with the expected
    /// input/output port shape. Guards the externally-queryable port layout
    /// the agent relies on via `list_node_factories`.
    #[test]
    fn node_info_ports_serialize() {
        let registry = test_registry();

        let serialized = serde_json::to_value(registry.list_nodes())
            .expect("NodeInfo list must serialize");
        let arr = serialized.as_array().expect("list_nodes serializes to an array");
        assert!(!arr.is_empty(), "registry should have registered nodes");

        // ldsc_rg has two typed input ports + one typed output — the strongest
        // shape to pin down.
        let rg = arr
            .iter()
            .find(|v| v["kind"] == "ldsc_rg")
            .expect("ldsc_rg kind present");
        let inputs = rg["ports"]["input_ports"]["ports"]
            .as_array()
            .expect("input ports array");
        assert_eq!(inputs.len(), 2, "ldsc_rg declares two input ports");
        assert!(inputs[0]["schema"].is_array(), "typed port exposes schema");
        let outputs = rg["ports"]["output_ports"]["ports"]
            .as_array()
            .expect("output ports array");
        assert_eq!(outputs.len(), 1, "ldsc_rg declares one output port");
    }
}
