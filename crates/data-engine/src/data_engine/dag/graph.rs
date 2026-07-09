//! The DAG data structure: a payload store + a structural index.

use std::collections::VecDeque;
use std::sync::Arc;

use datafusion::common::HashMap;
use datafusion::prelude::DataFrame;
use petgraph::Direction;
use petgraph::algo::{is_cyclic_directed, kosaraju_scc, toposort};
use petgraph::dot::Dot;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use tokio::sync::{Semaphore, mpsc};
use tracing::{debug, warn};

use crate::data_engine::dag::utils::{build_inputs, cascade_skip};

use super::error::DagError;
use super::runtime::{RunReport, RuntimeStatus, SchedulerConfig};
use super::{DagNode, NodeId};

pub type NamedDataFrames = HashMap<String, DataFrame>;

/// Module-local Result alias — every fallible operation in this module fails
/// with [`DagError`].
pub type Result<T> = std::result::Result<T, DagError>;

/// The workflow graph: payload store + connectivity index.
///
/// Node payloads (`Box<dyn DagNode>`) live in a [`HashMap`] keyed by id; a
/// lightweight [`petgraph`] directed graph mirrors both the connectivity and
/// edge metadata (see [`DependencyKind`]) so we get cycle detection,
/// topological sort, predecessor/successor queries, and edge iteration for
/// free. The two are decoupled on purpose — keeping payloads out of the graph
/// lets the scheduler `clone_box` a node into a spawned task without
/// fighting the graph's borrow.
#[derive(Default)]
pub struct DAG {
    /// Node payloads, keyed by id. Public so external tooling can introspect.
    pub nodes: HashMap<NodeId, Box<dyn DagNode>>,
    /// Connectivity index.
    graph: DiGraph<NodeId, ()>,
    pub id_to_idx: HashMap<NodeId, NodeIndex>,
    /// Per-node runtime status. Populated on [`Self::run`], queryable via [`Self::status`].
    pub statuses: HashMap<NodeId, RuntimeStatus>,
    outputs: HashMap<NodeId, NamedDataFrames>,
    errors: HashMap<NodeId, DagError>,
}

/// Messages a dispatched task sends back to the scheduler.
enum JobResult {
    Success {
        id: NodeId,
        outputs: NamedDataFrames,
    },
    Failed {
        id: NodeId,
        error: DagError,
    },
}

impl DAG {
    /// Query the runtime status of a node. Returns `None` if the DAG has never
    /// been run.
    pub fn status(&self, id: &str) -> Option<RuntimeStatus> {
        self.statuses.get(id).copied()
    }

    pub fn output(&self, id: &str) -> Option<NamedDataFrames> {
        self.outputs.get(id).cloned()
    }

    /// Remove all nodes, edges, statuses, outputs, and errors — a full reset.
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.graph.clear();
        self.id_to_idx.clear();
        self.statuses.clear();
        self.outputs.clear();
        self.errors.clear();
    }

    /// Reset all node statuses to [`RuntimeStatus::Pending`], preparing for a
    /// re-run.
    pub fn reset(&mut self) {
        for id in self.nodes.keys() {
            self.statuses.insert(id.clone(), RuntimeStatus::Pending);
        }
    }

    /// Execute every node of the DAG according to its dependencies.
    ///
    /// Uses [`DagNode::clone_box`] to copy node payloads into spawned tasks so
    /// the original nodes stay in the DAG for re-runs / iterative optimisation.
    pub async fn run(&mut self, cfg: &SchedulerConfig) -> Result<RunReport> {
        self.validate()?;
        // Topological order is computed mainly to validate the graph and to seed a
        // deterministic processing order for the ready queue.
        let _topo = self.topo_order()?;

        let all_ids = self.node_ids();

        // Precompute adjacency + per-node port assignment so the dispatch loop only
        // needs a single mutable borrow of `self`.
        let mut successors: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        // (predecessor id, resolved input port) per node, in declared edge order
        let mut incoming: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        // unresolved-predecessor count per node
        let mut pending: HashMap<NodeId, usize> = HashMap::new();
        for id in &all_ids {
            successors.insert(id.clone(), self.successors(id));
            pending.insert(id.clone(), self.predecessors(id).len());
            let inc = self.incoming_edges(id);
            incoming.insert(id.clone(), inc);
        }

        // Initialise runtime state.
        self.statuses.clear();
        for id in &all_ids {
            self.statuses.insert(id.clone(), RuntimeStatus::Pending);
        }
        // let mut outputs: HashMap<NodeId, Vec<DataFrame>> = HashMap::new();
        // let mut errors: HashMap<NodeId, DagError> = HashMap::new();

        let sem = Arc::new(Semaphore::new(cfg.max_concurrency.max(1)));
        let (tx, mut rx) = mpsc::channel::<JobResult>(all_ids.len().max(1));

        // Seed the ready queue with source nodes.
        let mut ready: VecDeque<NodeId> = all_ids
            .iter()
            .filter(|id| pending[*id] == 0)
            .cloned()
            .collect();
        let mut in_flight: usize = 0;

        loop {
            // Dispatch every currently-ready node.
            while let Some(id) = ready.pop_front() {
                if self.statuses[&id] != RuntimeStatus::Pending {
                    // Already skipped/finished by a cascade — don't dispatch.
                    continue;
                }
                let inputs = build_inputs(&id, &incoming, &self.outputs);
                // Borrow the node payload, then clone it into an owned Box so it
                // can be moved into the 'static future. The original stays in
                // `self` for re-runs / iterative optimisation.
                let Some(node_box) = self.get_node(&id).map(|n| n.clone_box()) else {
                    warn!(node = %id, "scheduler: node payload missing");
                    continue;
                };
                self.statuses.insert(id.clone(), RuntimeStatus::Running);
                in_flight += 1;
                let tx = tx.clone();
                let sem = sem.clone();
                let job_id = id.clone();
                tokio::spawn(async move {
                    let _permit = sem.acquire().await.ok();
                    let mut node = node_box;
                    let result = node.execute(&inputs).await;
                    match result {
                        Ok(outs) => {
                            let _ = tx
                                .send(JobResult::Success {
                                    id: job_id,
                                    outputs: outs,
                                })
                                .await;
                        }
                        Err(error) => {
                            warn!(node = %job_id, error = %error, "node failed");
                            let _ = tx.send(JobResult::Failed { id: job_id, error }).await;
                        }
                    }
                });
            }

            if in_flight == 0 {
                break;
            }

            // Block until at least one dispatched job reports back.
            let Some(msg) = rx.recv().await else {
                return Err(DagError::Schedule(
                    "result channel closed unexpectedly".into(),
                ));
            };
            in_flight -= 1;

            match msg {
                JobResult::Success { id, outputs: outs } => {
                    self.outputs.insert(id.clone(), outs);
                    self.statuses.insert(id.clone(), RuntimeStatus::Success);
                    debug!(node = %id, "node succeeded");
                    for succ in &successors[&id] {
                        let left = {
                            let c = pending.entry(succ.clone()).or_insert(0);
                            *c = c.saturating_sub(1);
                            *c
                        };
                        if left == 0 && self.statuses[succ] == RuntimeStatus::Pending {
                            ready.push_back(succ.clone());
                        }
                    }
                }
                JobResult::Failed { id, error } => {
                    self.statuses.insert(id.clone(), RuntimeStatus::Failed);
                    debug!(node = %id, error = %error, "node failed; cascading skip to descendants");
                    self.errors.insert(id.clone(), error);
                    cascade_skip(&id, &successors, &mut self.statuses, &mut ready);
                }
            }
        }

        let ok = !self
            .statuses
            .values()
            .any(|s| matches!(s, RuntimeStatus::Failed));

        Ok(RunReport {
            statuses: self.statuses.clone(),
            ok,
            errors: self.errors.drain().collect(),
        })
    }
}

impl DAG {
    /// Register a node under `id`. Errors if the id is already taken.
    pub fn add_node(&mut self, id: NodeId, node: Box<dyn DagNode>) -> Result<()> {
        if self.nodes.contains_key(&id) {
            return Err(DagError::DuplicateNode(id));
        }
        let idx = self.graph.add_node(id.clone());
        self.id_to_idx.insert(id.clone(), idx);
        self.nodes.insert(id, node);
        Ok(())
    }

    /// Add a dependency edge `from -> to`. Both endpoints must already exist.
    pub fn add_edge(&mut self, from: impl Into<NodeId>, to: impl Into<NodeId>) -> Result<()> {
        let from = from.into();
        let to = to.into();
        if !self.nodes.contains_key(&from) {
            return Err(DagError::UnknownNode(from));
        }
        if !self.nodes.contains_key(&to) {
            return Err(DagError::UnknownNode(to));
        }
        if let (Some(&a), Some(&b)) = (self.id_to_idx.get(&from), self.id_to_idx.get(&to)) {
            self.graph.add_edge(a, b, ());
        }
        Ok(())
    }

    pub fn delete_node(&mut self, id: &str) -> Result<()> {
        let target_node_idx = self
            .id_to_idx
            .get(id)
            .ok_or_else(|| DagError::UnknownNode(id.to_string()))?;
        let successors = self.successors(id);
        if !successors.is_empty() {
            return Err(DagError::Schedule(format!(
                "Cannot delete node `{id}`: the following successor node(s) still depend on it: [{}]. \
                 Remove those nodes (or their incoming edges) first.",
                successors.join(", ")
            )));
        }
        self.graph.remove_node(*target_node_idx);
        self.nodes.remove(id);
        self.id_to_idx.remove(id);
        self.statuses.remove(id);
        self.outputs.remove(id);
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

    /// Incoming edges for `id`, in insertion order. Returns predecessor ids.
    pub fn incoming_edges(&self, id: &str) -> Vec<NodeId> {
        let Some(&idx) = self.id_to_idx.get(id) else {
            return Vec::new();
        };
        self.graph
            .edges_directed(idx, Direction::Incoming)
            .map(|e| self.graph[e.source()].clone())
            .collect()
    }

    /// Fail if the graph has a cycle. Reports the offending nodes.
    pub fn validate(&self) -> Result<()> {
        if is_cyclic_directed(&self.graph) {
            return Err(DagError::Cycle(self.cycle_node_names()));
        }
        Ok(())
    }

    /// Topological order (predecessors before successors). Errors on a cycle.
    pub fn topo_order(&self) -> Result<Vec<NodeId>> {
        match toposort(&self.graph, None) {
            Ok(order) => Ok(order.iter().map(|i| self.graph[*i].clone()).collect()),
            Err(_) => Err(DagError::Cycle(self.cycle_node_names())),
        }
    }

    /// Borrow a node payload by id. Returns the trait object directly — no
    /// `Box` in the return type, since callers only want to call methods on
    /// the node (or take a fresh `Box<dyn DagNode>` themselves if they need
    /// ownership).
    pub fn get_node(&self, id: &str) -> Option<&dyn DagNode> {
        self.nodes.get(id).map(|b| b.as_ref())
    }

    /// Build a human-readable cycle path like `A → B → C → A` from the first
    /// strongly-connected component that contains a cycle.
    ///
    /// Uses DFS within the SCC to recover an actual cycle (not just the node set).
    fn cycle_node_names(&self) -> String {
        let sccs = kosaraju_scc(&self.graph);
        for scc in sccs {
            let cyclic = scc.len() > 1
                || scc
                    .first()
                    .map(|&i| self.graph.neighbors(i).any(|j| j == i))
                    .unwrap_or(false);
            if !cyclic {
                continue;
            }
            // Collect node ids in this SCC and map from NodeIndex → node id.
            let ids: Vec<String> = scc.iter().map(|&i| self.graph[i].clone()).collect();
            let idx_set: std::collections::HashSet<NodeIndex> = scc.iter().copied().collect();
            // DFS to find an actual cycle path within the SCC.
            if let Some(path) = self.find_cycle_path(&idx_set) {
                let names: Vec<&str> = path.iter().map(|&i| self.graph[i].as_str()).collect();
                return names.join(" → ");
            }
            // Fallback: list the SCC members (shouldn't happen for a cyclic SCC).
            return ids.join(", ");
        }
        String::from("<unknown>")
    }

    /// DFS within a known SCC to recover one concrete cycle path.
    ///
    /// Returns a vec of [`NodeIndex`] forming a cycle (first element == last).
    fn find_cycle_path(
        &self,
        idx_set: &std::collections::HashSet<NodeIndex>,
    ) -> Option<Vec<NodeIndex>> {
        // Try DFS from each node in the SCC until we find a back-edge.
        let start = *idx_set.iter().next()?;
        let mut stack: Vec<NodeIndex> = vec![start];
        let mut on_stack: std::collections::HashSet<NodeIndex> =
            std::collections::HashSet::from([start]);
        let mut visited: std::collections::HashSet<NodeIndex> =
            std::collections::HashSet::from([start]);

        loop {
            let &current = stack.last()?;
            // Look for a successor that is still on the stack (back-edge = cycle).
            for neighbor in self
                .graph
                .neighbors_directed(current, Direction::Outgoing)
            {
                if !idx_set.contains(&neighbor) {
                    continue;
                }
                if on_stack.contains(&neighbor) {
                    // Found a cycle: extract the portion from `neighbor` to end.
                    let cycle_start = stack.iter().position(|&n| n == neighbor).unwrap();
                    let mut path: Vec<NodeIndex> = stack[cycle_start..].to_vec();
                    path.push(neighbor);
                    return Some(path);
                }
                if !visited.contains(&neighbor) {
                    visited.insert(neighbor);
                    on_stack.insert(neighbor);
                    stack.push(neighbor);
                    break; // continue DFS from the pushed neighbor
                }
            }
            // If no unvisited successor was pushed, backtrack.
            if *stack.last().unwrap() == current {
                // We processed all neighbors without finding a cycle — pop and try next.
                on_stack.remove(&current);
                stack.pop();
                if stack.is_empty() {
                    return None;
                }
            }
        }
    }

    /// Render DAG topology into dot code
    pub fn to_dot(&self) -> String {
        format!("{:?}", Dot::with_config(&self.graph, &[]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_meta(id: &str) -> super::super::NodeMeta {
        super::super::NodeMeta::new(id)
    }

    fn get_diamond_dag() -> DAG {
        let mut dag = DAG::default();
        for id in ["a", "b", "c", "d"] {
            add(&mut dag, id);
        }
        dag.add_edge("a", "b").unwrap();
        dag.add_edge("a", "c").unwrap();
        dag.add_edge("b", "d").unwrap();
        dag.add_edge("c", "d").unwrap();
        dag
    }

    // A no-op node so tests can build a real DAG without touching IO.
    #[derive(Clone)]
    struct EchoNode(super::super::NodeMeta);
    #[async_trait::async_trait]
    impl super::super::DagNode for EchoNode {
        fn meta(&self) -> &super::super::NodeMeta {
            &self.0
        }
        fn clone_box(&self) -> Box<dyn super::super::DagNode> {
            Box::new((*self).clone())
        }
        async fn execute(
            &mut self,
            _inputs: &[super::super::NodeInput],
        ) -> std::result::Result<NamedDataFrames, super::super::DagError> {
            Ok(HashMap::new())
        }
    }

    fn add(dag: &mut DAG, id: &str) {
        dag.add_node(id.into(), Box::new(EchoNode(dummy_meta(id))))
            .unwrap();
    }

    #[test]
    fn topo_order_diamond() {
        let dag = get_diamond_dag();
        let order = dag.topo_order().unwrap();
        dbg!(&order);
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
        dag.add_edge("x", "y").unwrap();
        dag.add_edge("y", "x").unwrap();
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
            dag.add_edge("a", "ghost"),
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
        dag.add_edge("src", "a").unwrap();
        dag.add_edge("src", "b").unwrap();

        assert_eq!(dag.predecessors("a").len(), 1);
        assert_eq!(dag.predecessors("a")[0], "src");
        let mut succ = dag.successors("src");
        succ.sort_unstable();
        assert_eq!(succ, vec!["a", "b"]);
        let inc = dag.incoming_edges("a");
        assert_eq!(inc.len(), 1);
    }

    #[test]
    fn render_into_dot() {
        let dag = get_diamond_dag();
        let dot = dag.to_dot();

        // DOT output must be a digraph declaration
        assert!(
            dot.contains("digraph"),
            "to_dot output should be a digraph declaration"
        );

        // All 4 diamond nodes must appear as labels in the output
        for node_id in ["a", "b", "c", "d"] {
            assert!(
                dot.contains(node_id),
                "DOT output should contain node '{node_id}'"
            );
        }

        // 4 edges: a→b, a→c, b→d, c→d — petgraph uses "N -> M" notation
        assert!(dot.contains("->"), "DOT output should contain directed edges");

        // Smoke-check: non-trivial output (a 4-node DAG should be > 20 chars)
        assert!(dot.len() > 20, "DOT output seems too short, got: {dot}");
    }
}
