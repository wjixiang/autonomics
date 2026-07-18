use std::sync::Arc;

use datafusion::{common::HashMap, prelude::SessionContext};
use datalake::Datalake;

use super::error::{Error, Result};

use crate::data_engine::dag::DagNode;

pub struct PortTopology {}

pub trait NodeFactory: Send + Sync {
    fn kind(&self) -> &'static str;
    fn spec_schema(&self) -> schemars::Schema;
    fn build(&self, spec: serde_json::Value, node_ctx: NodeCtx) -> Result<Box<dyn DagNode>>;
    fn input_topology(&self) -> PortTopology;
    fn output_topology(&self) -> PortTopology;
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

pub trait NodeSpec {
    fn schema(&self) -> schemars::Schema;
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
    pub fn new(ctx: SessionContext, datalake: Arc<Datalake>) -> Self {
        let node_ctx = NodeCtx {
            session: ctx,
            datalake,
        };

        Self {
            node_ctx,
            nodes: Default::default(),
        }
    }

    // pub fn get_node_factory(&self, node_kind: &str) -> Result<Box<dyn NodeFactory>> {
    //     self.nodes
    //         .get(node_kind)
    //         .ok_or(Error::FactoryNotFound {
    //             kind: node_kind.to_string(),
    //         })
    //         .cloned()
    // }

    pub fn build_node(&self, node_kind: &str, spec: serde_json::Value) -> Result<Box<dyn DagNode>> {
        let node_factory = self.nodes.get(node_kind).ok_or(Error::FactoryNotFound {
            kind: node_kind.to_string(),
        })?;
        let node = node_factory.build(spec, self.node_ctx.clone())?;
        Ok(node)
    }
}
