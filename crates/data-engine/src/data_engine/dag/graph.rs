//! The DAG data structure: a payload store + a structural index.
//!
//! Node payloads (`Box<dyn DagNode>`) live in a [`HashMap`] keyed by id; a
//! lightweight [`petgraph`] directed graph mirrors only the connectivity so we
//! get cycle detection, topological sort, and predecessor/successor queries for
//! free. The two are decoupled on purpose — keeping payloads out of the graph
//! lets the scheduler `take` a node out of the map and move it into a spawned
//! task without fighting the graph's borrow.

use datafusion::common::HashMap;
use petgraph::Direction;
use petgraph::algo::{is_cyclic_directed, kosaraju_scc, toposort};
use petgraph::graph::{DiGraph, NodeIndex};

use super::error::DagError;
use super::{DagNode, NodeId};

/// How an edge's data flows between two nodes. `OneToOne` is the streaming
/// default; `Shuffle` is reserved for future partitioned fan-out.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum DependencyKind {
    #[default]
    OneToOne,
    Shuffle,
}

/// A declared dependency `from -> to`. `port` is the name under which the
/// upstream output is registered for the downstream node (e.g. the table name a
/// `SqlNode` references); `None` lets the scheduler assign a positional default.
#[derive(Debug, Clone)]
pub struct Edge {
    pub from: NodeId,
    pub to: NodeId,
    pub kind: DependencyKind,
    pub port: Option<String>,
}

impl Edge {
    pub fn new(from: impl Into<NodeId>, to: impl Into<NodeId>) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
            kind: DependencyKind::default(),
            port: None,
        }
    }

    pub fn with_port(mut self, port: impl Into<String>) -> Self {
        self.port = Some(port.into());
        self
    }
}

/// The workflow graph: payload store + connectivity index.
#[derive(Default)]
pub struct DAG {
    /// Node payloads, keyed by id. Public so external tooling can introspect.
    pub nodes: HashMap<NodeId, Box<dyn DagNode>>,
    /// Declared edges, in insertion order. Public for introspection.
    pub edges: Vec<Edge>,
    /// Connectivity index (id-only, no payload).
    graph: DiGraph<NodeId, ()>,
    id_to_idx: HashMap<NodeId, NodeIndex>,
}

impl DAG {
    /// Register a node under `id`. Errors if the id is already taken.
    pub fn add_node(&mut self, id: NodeId, node: Box<dyn DagNode>) -> Result<(), DagError> {
        if self.nodes.contains_key(&id) {
            return Err(DagError::DuplicateNode(id));
        }
        let idx = self.graph.add_node(id.clone());
        self.id_to_idx.insert(id.clone(), idx);
        self.nodes.insert(id, node);
        Ok(())
    }

    /// Add a dependency edge. Both endpoints must already exist.
    pub fn add_edge(&mut self, edge: Edge) -> Result<(), DagError> {
        if !self.nodes.contains_key(&edge.from) {
            return Err(DagError::UnknownNode(edge.from));
        }
        if !self.nodes.contains_key(&edge.to) {
            return Err(DagError::UnknownNode(edge.to));
        }
        if let (Some(&a), Some(&b)) = (self.id_to_idx.get(&edge.from), self.id_to_idx.get(&edge.to))
        {
            self.graph.add_edge(a, b, ());
        }
        self.edges.push(edge);
        Ok(())
    }

    /// All node ids, in graph (arbitrary) order.
    pub fn node_ids(&self) -> Vec<NodeId> {
        self.nodes.keys().cloned().collect()
    }

    /// Direct predecessors of `id`.
    pub fn predecessors(&self, id: &str) -> Vec<NodeId> {
        let Some(&idx) = self.id_to_idx.get(id) else {
            return Vec::new();
        };
        self.graph
            .neighbors_directed(idx, Direction::Incoming)
            .map(|i| self.graph[i].clone())
            .collect()
    }

    /// Direct successors of `id`.
    pub fn successors(&self, id: &str) -> Vec<NodeId> {
        let Some(&idx) = self.id_to_idx.get(id) else {
            return Vec::new();
        };
        self.graph
            .neighbors_directed(idx, Direction::Outgoing)
            .map(|i| self.graph[i].clone())
            .collect()
    }

    /// Incoming edges for `id`, in insertion order. Used by the scheduler to
    /// assemble a node's `inputs` with correct port names.
    pub fn incoming_edges(&self, id: &str) -> Vec<&Edge> {
        self.edges.iter().filter(|e| e.to == id).collect()
    }

    /// Fail if the graph has a cycle. Reports the offending nodes.
    pub fn validate(&self) -> Result<(), DagError> {
        if is_cyclic_directed(&self.graph) {
            return Err(DagError::Cycle(self.cycle_node_names()));
        }
        Ok(())
    }

    /// Topological order (predecessors before successors). Errors on a cycle.
    pub fn topo_order(&self) -> Result<Vec<NodeId>, DagError> {
        match toposort(&self.graph, None) {
            Ok(order) => Ok(order.iter().map(|i| self.graph[*i].clone()).collect()),
            Err(_) => Err(DagError::Cycle(self.cycle_node_names())),
        }
    }

    /// Remove and return a node payload by id, so it can be moved into a task.
    /// Leaves the connectivity index untouched (the scheduler precomputes the
    /// adjacency it needs before dispatch).
    pub fn take_node(&mut self, id: &str) -> Option<Box<dyn DagNode>> {
        self.nodes.remove(id)
    }

    /// Comma-separated names of the nodes participating in a cycle, pulled from
    /// the first non-trivial strongly-connected component.
    fn cycle_node_names(&self) -> String {
        let sccs = kosaraju_scc(&self.graph);
        for scc in sccs {
            let cyclic = scc.len() > 1
                || scc
                    .first()
                    .map(|&i| self.graph.neighbors(i).any(|j| j == i))
                    .unwrap_or(false);
            if cyclic {
                return scc
                    .into_iter()
                    .map(|i| self.graph[i].clone())
                    .collect::<Vec<_>>()
                    .join(", ");
            }
        }
        String::from("<unknown>")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_meta(id: &str) -> super::super::NodeMeta {
        use datafusion::prelude::SessionContext;
        use std::sync::Arc;
        super::super::NodeMeta::new(id, Arc::new(SessionContext::new()))
    }

    // A no-op node so tests can build a real DAG without touching IO.
    struct EchoNode(super::super::NodeMeta);
    #[async_trait::async_trait]
    impl super::super::DagNode for EchoNode {
        fn meta(&self) -> &super::super::NodeMeta {
            &self.0
        }
        async fn execute(
            &mut self,
            _inputs: &[super::super::NodeInput],
        ) -> Result<Vec<datafusion::prelude::DataFrame>, super::super::DagError> {
            Ok(vec![])
        }
    }

    fn add(dag: &mut DAG, id: &str) {
        dag.add_node(id.into(), Box::new(EchoNode(dummy_meta(id))))
            .unwrap();
    }

    #[test]
    fn topo_order_diamond() {
        let mut dag = DAG::default();
        for id in ["a", "b", "c", "d"] {
            add(&mut dag, id);
        }
        dag.add_edge(Edge::new("a", "b")).unwrap();
        dag.add_edge(Edge::new("a", "c")).unwrap();
        dag.add_edge(Edge::new("b", "d")).unwrap();
        dag.add_edge(Edge::new("c", "d")).unwrap();

        let order = dag.topo_order().unwrap();
        let pos = |id: &str| order.iter().position(|x| x == id).unwrap();
        assert!(pos("a") < pos("b"));
        assert!(pos("a") < pos("c"));
        assert!(pos("b") < pos("d"));
        assert!(pos("c") < pos("d"));
    }

    #[test]
    fn cycle_rejected() {
        let mut dag = DAG::default();
        add(&mut dag, "x");
        add(&mut dag, "y");
        dag.add_edge(Edge::new("x", "y")).unwrap();
        dag.add_edge(Edge::new("y", "x")).unwrap();
        let err = dag.validate().unwrap_err();
        assert!(matches!(err, DagError::Cycle(_)), "{err:?}");
        let err = dag.topo_order().unwrap_err();
        assert!(matches!(err, DagError::Cycle(_)));
        dbg!(err);
    }

    #[test]
    fn unknown_and_duplicate() {
        let mut dag = DAG::default();
        add(&mut dag, "a");
        // edge to missing node
        assert!(matches!(
            dag.add_edge(Edge::new("a", "ghost")),
            Err(DagError::UnknownNode(_))
        ));
        // duplicate id
        assert!(matches!(
            dag.add_node("a".into(), Box::new(EchoNode(dummy_meta("a")))),
            Err(DagError::DuplicateNode(_))
        ));
    }

    #[test]
    fn predecessors_and_incoming() {
        let mut dag = DAG::default();
        for id in ["src", "a", "b"] {
            add(&mut dag, id);
        }
        dag.add_edge(Edge::new("src", "a").with_port("s1")).unwrap();
        dag.add_edge(Edge::new("src", "b")).unwrap();

        assert_eq!(dag.predecessors("a").len(), 1);
        assert_eq!(dag.predecessors("a")[0], "src");
        let mut succ = dag.successors("src");
        succ.sort_unstable();
        assert_eq!(succ, vec!["a", "b"]);
        let inc = dag.incoming_edges("a");
        assert_eq!(inc.len(), 1);
        assert_eq!(inc[0].port.as_deref(), Some("s1"));
    }
}
