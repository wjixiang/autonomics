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

use super::super::nodes::sink::SinkNode;
use super::error::DagError;
use super::runtime::{NodeReport, RunReport, RuntimeStatus, SchedulerConfig};
use super::{DagNode, NodeId};

/// Output DataFrames keyed by output port index.
pub type PortOutputs = HashMap<u8, DataFrame>;

/// Metadata attached to every edge in the graph: which output port of the
/// source feeds which input port of the target. Exactly one [`DataFrame`] flows
/// along each edge.
#[derive(Debug, Clone)]
pub struct EdgeLabel {
    /// Output port name on the `from` node.
    pub from_port: u8,
    /// Input port name on the `to` node.
    pub to_port: u8,
}

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
    /// Connectivity index (edge weight carries the port label).
    graph: DiGraph<NodeId, EdgeLabel>,
    pub id_to_idx: HashMap<NodeId, NodeIndex>,
    /// Per-node runtime status. Populated on [`Self::run`], queryable via [`Self::status`].
    pub statuses: HashMap<NodeId, RuntimeStatus>,
    outputs: HashMap<NodeId, PortOutputs>,
    errors: HashMap<NodeId, DagError>,
}

/// Messages a dispatched task sends back to the scheduler.
enum JobResult {
    Success {
        id: NodeId,
        outputs: PortOutputs,
        duration: std::time::Duration,
    },
    Failed {
        id: NodeId,
        error: DagError,
        duration: std::time::Duration,
    },
}

impl DAG {
    /// Query the runtime status of a node. Returns `None` if the DAG has never
    /// been run.
    pub fn status(&self, id: &str) -> Option<RuntimeStatus> {
        self.statuses.get(id).copied()
    }

    pub fn output(&self, id: &str) -> Option<PortOutputs> {
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
        // (predecessor id, edge port label) per node, in declared edge order
        let mut incoming: HashMap<NodeId, Vec<(NodeId, super::graph::EdgeLabel)>> = HashMap::new();
        // unresolved-predecessor count per node
        let mut pending: HashMap<NodeId, usize> = HashMap::new();
        for id in &all_ids {
            successors.insert(id.clone(), self.successors(id));
            pending.insert(id.clone(), self.predecessors(id).len());
            let inc = self.incoming_edges_with_ports(id);
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

        // Per-node execution duration and skip root-cause tracking.
        let mut durations: HashMap<NodeId, std::time::Duration> = HashMap::new();
        let mut skipped_because: HashMap<NodeId, NodeId> = HashMap::new();

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
                    let start = std::time::Instant::now();
                    let result = node.execute(&inputs).await;
                    let duration = start.elapsed();
                    match result {
                        Ok(outs) => {
                            let _ = tx
                                .send(JobResult::Success {
                                    id: job_id,
                                    outputs: outs,
                                    duration,
                                })
                                .await;
                        }
                        Err(error) => {
                            warn!(node = %job_id, error = %error, "node failed");
                            let _ = tx
                                .send(JobResult::Failed {
                                    id: job_id,
                                    error,
                                    duration,
                                })
                                .await;
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
                JobResult::Success {
                    id,
                    outputs: outs,
                    duration,
                } => {
                    self.outputs.insert(id.clone(), outs);
                    self.statuses.insert(id.clone(), RuntimeStatus::Success);
                    durations.insert(id.clone(), duration);
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
                JobResult::Failed {
                    id,
                    error,
                    duration,
                } => {
                    self.statuses.insert(id.clone(), RuntimeStatus::Failed);
                    durations.insert(id.clone(), duration);
                    debug!(node = %id, error = %error, "node failed; cascading skip to descendants");
                    self.errors.insert(id.clone(), error);
                    cascade_skip(
                        &id,
                        &successors,
                        &mut self.statuses,
                        &mut ready,
                        &mut skipped_because,
                    );
                }
            }
        }

        let ok = !self
            .statuses
            .values()
            .any(|s| matches!(s, RuntimeStatus::Failed));

        // Build per-node reports for the agent-friendly result.
        let node_reports = self
            .build_node_reports(&all_ids, &durations, &skipped_because)
            .await;

        Ok(RunReport {
            ok,
            nodes: node_reports,
            statuses: self.statuses.clone(),
            errors: self.errors.drain().collect(),
        })
    }

    /// Build per-node [`NodeReport`] summaries from the execution state
    /// available after the scheduler loop.
    async fn build_node_reports(
        &self,
        all_ids: &[NodeId],
        durations: &HashMap<NodeId, std::time::Duration>,
        skipped_because: &HashMap<NodeId, NodeId>,
    ) -> Vec<NodeReport> {
        // We collect all futures for row counts at once to avoid sequential awaits.
        let mut count_futs = Vec::new();
        for id in all_ids {
            if let Some(dfs) = self.outputs.get(id) {
                // Take the first output port's DataFrame for the row count.
                if let Some((_name, df)) = dfs.iter().next() {
                    count_futs.push((id.clone(), df.clone().count()));
                }
            }
        }

        let counts: HashMap<NodeId, usize> = {
            let mut map = HashMap::new();
            for (id, fut) in count_futs {
                map.insert(id, fut.await.unwrap_or(0));
            }
            map
        };

        all_ids
            .iter()
            .map(|id| {
                let status = self
                    .statuses
                    .get(id)
                    .copied()
                    .unwrap_or(RuntimeStatus::Pending);
                let node_type = self
                    .nodes
                    .get(id)
                    .map(|n| n.node_type())
                    .unwrap_or("unknown")
                    .to_string();

                // Extract output schema from the first output port's DataFrame.
                let output_schema = self.outputs.get(id).and_then(|dfs| {
                    dfs.values().next().map(|df| {
                        df.schema()
                            .fields()
                            .iter()
                            .map(|f| (f.name().clone(), f.data_type().to_string()))
                            .collect()
                    })
                });

                let output_rows = counts.get(id).copied();
                let elapsed_ms = durations.get(id).map(|d| d.as_millis() as u64);

                // Extract sink path via downcast.
                let sink_path = self
                    .nodes
                    .get(id)
                    .and_then(|n| n.as_any().downcast_ref::<SinkNode>())
                    .and_then(|sn| match sn.sink() {
                        super::super::Sink::File { path, .. } => Some(path.clone()),
                        _ => None,
                    });

                let error = self.errors.get(id).map(|e| e.to_report());
                let skipped_because = skipped_because.get(id).cloned();

                NodeReport {
                    id: id.clone(),
                    status,
                    node_type,
                    output_schema,
                    output_rows,
                    elapsed_ms,
                    sink_path,
                    error,
                    skipped_because,
                }
            })
            .collect()
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

    /// Add an edge from `from`'s `from_port` output port to `to`'s `to_port`
    /// input port. Port existence and the strict 1:1 / completeness constraints
    /// are checked later in [`Self::validate`].
    pub fn add_edge(
        &mut self,
        from: impl Into<NodeId>,
        to: impl Into<NodeId>,
        from_port: u8,
        to_port: u8,
    ) -> Result<()> {
        let from = from.into();
        let to = to.into();
        self.resolve_nodes(&from, &to)?;
        if let (Some(&a), Some(&b)) = (self.id_to_idx.get(&from), self.id_to_idx.get(&to)) {
            self.graph.add_edge(a, b, EdgeLabel { from_port, to_port });
        }
        Ok(())
    }

    // /// Add an edge connecting `from`'s `from_port` (an output port) to `to`'s
    // /// `to_port` (an input port). Port existence and 1:1 constraints are checked
    // /// in [`Self::validate`].
    // pub fn add_edge_port(
    //     &mut self,
    //     from: impl Into<NodeId>,
    //     from_port: impl Into<String>,
    //     to: impl Into<NodeId>,
    //     to_port: impl Into<String>,
    // ) -> Result<()> {
    //     let from = from.into();
    //     let to = to.into();
    //     self.resolve_nodes(&from, &to)?;
    //     if let (Some(&a), Some(&b)) = (self.id_to_idx.get(&from), self.id_to_idx.get(&to)) {
    //         self.graph.add_edge(
    //             a,
    //             b,
    //             EdgeLabel {
    //                 from_port: from_port.into(),
    //                 to_port: to_port.into(),
    //             },
    //         );
    //     }
    //     Ok(())
    // }
    //
    /// Validate that `from` and `to` refer to existing nodes.
    fn resolve_nodes(&self, from: &str, to: &str) -> Result<()> {
        if !self.nodes.contains_key(from) {
            return Err(DagError::UnknownNode(from.to_string()));
        }
        if !self.nodes.contains_key(to) {
            return Err(DagError::UnknownNode(to.to_string()));
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
        self.incoming_edges_with_ports(id)
            .into_iter()
            .map(|(nid, _)| nid)
            .collect()
    }

    /// Incoming edges for `id` with their port labels, in insertion order.
    /// Returns `(predecessor_id, edge_label)` pairs.
    pub fn incoming_edges_with_ports(&self, id: &str) -> Vec<(NodeId, EdgeLabel)> {
        let Some(&idx) = self.id_to_idx.get(id) else {
            return Vec::new();
        };
        self.graph
            .edges_directed(idx, Direction::Incoming)
            .map(|e| (self.graph[e.source()].clone(), e.weight().clone()))
            .collect()
    }

    /// Validate the graph: cycles, port wiring, and schema compatibility.
    ///
    /// Checks (in order):
    /// 1. No cycles.
    /// 2. Every edge references ports that exist on its endpoints.
    /// 3. The default-port `add_edge` form was only used on single-port nodes.
    /// 4. Each input port has at most one incoming edge (strict 1:1).
    /// 5. Every declared input port has exactly one incoming edge.
    /// 6. Where both endpoints declare a schema, the output schema covers the
    ///    input schema's required fields with compatible types.
    pub fn validate(&self) -> Result<()> {
        if is_cyclic_directed(&self.graph) {
            return Err(DagError::Cycle(self.cycle_node_names()));
        }
        self.validate_port_wiring()?;
        self.validate_schemas()?;
        Ok(())
    }

    /// Port existence, default-edge disambiguation, strict-1:1, and completeness.
    fn validate_port_wiring(&self) -> Result<()> {
        use datafusion::common::HashMap;
        // node id -> (input port index -> incoming edge count)
        let mut incoming_counts: HashMap<NodeId, HashMap<u8, usize>> = HashMap::new();
        for id in self.nodes.keys() {
            incoming_counts.insert(id.clone(), HashMap::new());
        }

        for edge in self.graph.edge_references() {
            let from = self.graph[edge.source()].clone();
            let to = self.graph[edge.target()].clone();
            let label = edge.weight();
            let from_meta = self.nodes[&from].meta();
            let to_meta = self.nodes[&to].meta();

            // Port existence.
            if from_meta.output_port(label.from_port).is_none() {
                return Err(DagError::PortNotFound {
                    node: from,
                    port: label.from_port,
                    direction: "output",
                });
            }

            if to_meta.is_fixed_input() {
                if to_meta.input_port(label.to_port).is_none() {
                    return Err(DagError::PortNotFound {
                        node: to.clone(),
                        port: label.to_port,
                        direction: "input",
                    });
                }
            }

            // Strict 1:1 on the input port.
            let counter = incoming_counts.get_mut(&to).unwrap();
            let entry = counter.entry(label.to_port).or_insert(0);
            *entry += 1;
            if *entry > 1 {
                return Err(DagError::PortOverconnected {
                    node: to,
                    port: label.to_port,
                });
            }
        }

        // Completeness: every declared input port must have exactly one edge.
        for (id, counts) in &incoming_counts {
            let meta = self.nodes[id].meta();
            for port in meta.input_ports().iter() {
                let n = counts.get(&port.index).copied().unwrap_or(0);
                if n == 0 {
                    return Err(DagError::PortDisconnected {
                        node: id.clone(),
                        port: port.index,
                    });
                }
            }
        }
        Ok(())
    }

    /// Schema compatibility between connected ports (skipped when either side's
    /// schema is `None`).
    fn validate_schemas(&self) -> Result<()> {
        for edge in self.graph.edge_references() {
            let from = self.graph[edge.source()].clone();
            let to = self.graph[edge.target()].clone();
            let label = edge.weight();
            let from_port = self.nodes[&from].meta().output_port(label.from_port);
            let to_port = self.nodes[&to].meta().input_port(label.to_port);
            let (Some(fp), Some(tp)) = (from_port, to_port) else {
                continue;
            };
            let (Some(out_schema), Some(in_schema)) = (fp.schema.as_ref(), tp.schema.as_ref())
            else {
                continue; // unknown on either side → skip
            };
            if let Err(reason) = schema_compatible(out_schema, in_schema) {
                return Err(DagError::SchemaMismatch {
                    from_node: from,
                    from_port: label.from_port,
                    to_node: to,
                    to_port: label.to_port,
                    reason,
                });
            }
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
            for neighbor in self.graph.neighbors_directed(current, Direction::Outgoing) {
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

/// Check that every field required by `input` is present in `output` with a
/// compatible type.
///
/// Compatibility rule: the output schema must contain, by name, every field the
/// input schema declares, and the types must match exactly. (Stricter than
/// "subtype"; deliberately conservative — if a transform needs looser rules it
/// can leave the port schema `None`.)
fn schema_compatible(
    output: &arrow_schema::SchemaRef,
    input: &arrow_schema::SchemaRef,
) -> std::result::Result<(), String> {
    use std::collections::HashMap as StdHashMap;
    let out_fields: StdHashMap<&str, &arrow_schema::Field> = output
        .fields()
        .iter()
        .map(|f| (f.name().as_str(), f.as_ref()))
        .collect();
    for in_field in input.fields() {
        match out_fields.get(in_field.name().as_str()) {
            None => {
                return Err(format!(
                    "input requires column `{}` which is absent from output",
                    in_field.name()
                ));
            }
            Some(out_field) if out_field.data_type() != in_field.data_type() => {
                return Err(format!(
                    "column `{}` type mismatch: output {:?} vs input {:?}",
                    in_field.name(),
                    out_field.data_type(),
                    in_field.data_type()
                ));
            }
            _ => {}
        }
    }
    Ok(())
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
        dag.add_edge("a", "b", 0, 0).unwrap();
        dag.add_edge("a", "c", 0, 0).unwrap();
        dag.add_edge("b", "d", 0, 0).unwrap();
        dag.add_edge("c", "d", 0, 0).unwrap();
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
        fn node_type(&self) -> &str {
            "echo"
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        async fn execute(
            &mut self,
            _inputs: &[super::super::NodeInput],
        ) -> std::result::Result<PortOutputs, super::super::DagError> {
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
        dag.add_edge("x", "y", 0, 0).unwrap();
        dag.add_edge("y", "x", 0, 0).unwrap();
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
            dag.add_edge("a", "ghost", 0, 0),
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
        dag.add_edge("src", "a", 0, 0).unwrap();
        dag.add_edge("src", "b", 0, 0).unwrap();

        assert_eq!(dag.predecessors("a").len(), 1);
        assert_eq!(dag.predecessors("a")[0], "src");
        let mut succ = dag.successors("src");
        succ.sort_unstable();
        assert_eq!(succ, vec!["a", "b"]);
        let inc = dag.incoming_edges("a");
        assert_eq!(inc.len(), 1);
    }

    #[test]
    fn default_edge_uses_default_ports() {
        let mut dag = DAG::default();
        for id in ["src", "a"] {
            add(&mut dag, id);
        }
        dag.add_edge("src", "a", 0, 0).unwrap();

        let edges = dag.incoming_edges_with_ports("a");
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].0, "src");
        assert_eq!(edges[0].1.from_port, 0);
        assert_eq!(edges[0].1.to_port, 0);
    }

    #[test]
    fn explicit_edge_ports() {
        let mut dag = DAG::default();
        for id in ["x", "y"] {
            add(&mut dag, id);
        }
        dag.add_edge("x", "y", 1, 0).unwrap();

        let edges = dag.incoming_edges_with_ports("y");
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].1.from_port, 1);
        assert_eq!(edges[0].1.to_port, 0);
    }

    #[test]
    fn diamond_edge_ports() {
        let dag = get_diamond_dag();
        // Diamond: a→b, a→c, b→d, c→d — all default ports.
        let edges_d = dag.incoming_edges_with_ports("d");
        assert_eq!(edges_d.len(), 2);
        let from_nodes: Vec<&str> = edges_d.iter().map(|(n, _)| n.as_str()).collect();
        assert!(from_nodes.contains(&"b"));
        assert!(from_nodes.contains(&"c"));
        for (_, e) in &edges_d {
            assert_eq!(e.from_port, 0);
            assert_eq!(e.to_port, 0);
        }
    }

    /// A no-op node with caller-supplied port topology (for schema/port tests).
    #[derive(Clone)]
    struct PortedNode(super::super::NodeMeta);
    #[async_trait::async_trait]
    impl super::super::DagNode for PortedNode {
        fn meta(&self) -> &super::super::NodeMeta {
            &self.0
        }
        fn clone_box(&self) -> Box<dyn super::super::DagNode> {
            Box::new((*self).clone())
        }
        fn node_type(&self) -> &str {
            "ported"
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        async fn execute(
            &mut self,
            _inputs: &[super::super::NodeInput],
        ) -> std::result::Result<PortOutputs, super::super::DagError> {
            Ok(HashMap::new())
        }
    }

    fn make_schema(cols: &[(&str, arrow_schema::DataType)]) -> arrow_schema::SchemaRef {
        std::sync::Arc::new(arrow_schema::Schema::new(
            cols.iter()
                .map(|(n, t)| arrow_schema::Field::new(*n, t.clone(), true))
                .collect::<Vec<_>>(),
        ))
    }

    #[test]
    fn schema_compatible_passes() {
        // Output schema is a superset of input schema → OK.
        let out = make_schema(&[
            ("a", arrow_schema::DataType::Int32),
            ("b", arrow_schema::DataType::Utf8),
        ]);
        let inp = make_schema(&[("a", arrow_schema::DataType::Int32)]);
        assert!(schema_compatible(&out, &inp).is_ok());
    }

    #[test]
    fn schema_mismatch_rejected() {
        // Input requires a column the output lacks, and a type differs.
        let out = make_schema(&[("a", arrow_schema::DataType::Int32)]);
        let inp = make_schema(&[
            ("a", arrow_schema::DataType::Int64),
            ("b", arrow_schema::DataType::Utf8),
        ]);
        let err = schema_compatible(&out, &inp).unwrap_err();
        assert!(err.contains("a"), "{err}");
    }

    #[test]
    fn validate_schema_mismatch_between_ports() {
        use crate::data_engine::nodes::Port;
        let mut dag = DAG::default();
        let out_schema = make_schema(&[("a", arrow_schema::DataType::Int32)]);
        let in_schema = make_schema(&[("a", arrow_schema::DataType::Int64)]);
        dag.add_node(
            "src".into(),
            Box::new(PortedNode(
                super::super::NodeMeta::new("src")
                    .with_inputs(vec![])
                    .with_outputs(vec![Port::typed(0, out_schema)]),
            )),
        )
        .unwrap();
        dag.add_node(
            "dst".into(),
            Box::new(PortedNode(
                super::super::NodeMeta::new("dst")
                    .with_inputs(vec![Port::typed(0, in_schema)])
                    .with_outputs(vec![]),
            )),
        )
        .unwrap();
        dag.add_edge("src", "dst", 0, 0).unwrap();
        let err = dag.validate().unwrap_err();
        assert!(
            matches!(err, DagError::SchemaMismatch { ref from_node, ref to_port, .. } if from_node == "src" && *to_port == 0),
            "expected SchemaMismatch, got {err:?}"
        );
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
        assert!(
            dot.contains("->"),
            "DOT output should contain directed edges"
        );

        // Smoke-check: non-trivial output (a 4-node DAG should be > 20 chars)
        assert!(dot.len() > 20, "DOT output seems too short, got: {dot}");
    }
}
