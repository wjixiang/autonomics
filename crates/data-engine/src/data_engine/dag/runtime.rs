//! Async readiness scheduler and runtime state.
//!
//! The scheduler runs the DAG by repeatedly dispatching *ready* nodes (those
//! whose predecessors are all complete), bounded by a concurrency semaphore,
//! and threading each node's outputs to its successors. Node payloads are moved
//! into spawned tasks; runtime progress lives here in [`RuntimeState`], separate
//! from the nodes themselves.
//!
//! Failure model: a node failure does not abort the run. Its transitive
//! descendants are marked [`RuntimeStatus::Skipped`] while independent branches
//! keep executing. The final [`RunReport::ok`] is `false` iff any node failed.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use datafusion::prelude::DataFrame;
use tokio::sync::{mpsc, Semaphore};
use tracing::{debug, warn};

use super::error::DagError;
use super::graph::DAG;
use super::{NodeInput, NodeId};

/// Per-node runtime lifecycle state tracked by the scheduler.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeStatus {
    #[default]
    Pending,
    Ready,
    Running,
    Success,
    Failed,
    /// Not run because an upstream predecessor failed.
    Skipped,
}

/// Scheduler tuning knobs.
#[derive(Clone)]
pub struct SchedulerConfig {
    /// Maximum number of nodes running concurrently (semaphore permits).
    pub max_concurrency: usize,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        let cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        Self {
            max_concurrency: cpus,
        }
    }
}

/// Result of a single `run_dag` invocation: the final status of every node and
/// whether the whole run succeeded.
#[derive(Debug)]
pub struct RunReport {
    pub statuses: HashMap<NodeId, RuntimeStatus>,
    pub errors: HashMap<NodeId, DagError>,
    pub ok: bool,
}

impl RunReport {
    pub fn status(&self, id: &str) -> Option<RuntimeStatus> {
        self.statuses.get(id).copied()
    }

    /// The error that failed `id`, if any.
    pub fn error(&self, id: &str) -> Option<&DagError> {
        self.errors.get(id)
    }
}

/// Messages a dispatched task sends back to the scheduler.
enum JobResult {
    Success {
        id: NodeId,
        outputs: Vec<DataFrame>,
    },
    Failed {
        id: NodeId,
        error: DagError,
    },
}

/// Execute every node of `dag` according to its dependencies.
///
/// Consumes node payloads out of `dag` (via [`DAG::take_node`]) as they are
/// dispatched; the graph connectivity is left intact.
pub async fn run_dag(dag: &mut DAG, cfg: &SchedulerConfig) -> Result<RunReport, DagError> {
    dag.validate()?;
    // Topological order is computed mainly to validate the graph and to seed a
    // deterministic processing order for the ready queue.
    let _topo = dag.topo_order()?;

    let all_ids = dag.node_ids();

    // Precompute adjacency + per-node port assignment so the dispatch loop only
    // needs a single mutable borrow of `dag` (for `take_node`).
    // successor list
    let mut successors: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
    // (predecessor id, resolved input port) per node, in declared edge order
    let mut incoming: HashMap<NodeId, Vec<(NodeId, String)>> = HashMap::new();
    // unresolved-predecessor count per node
    let mut pending: HashMap<NodeId, usize> = HashMap::new();
    for id in &all_ids {
        successors.insert(id.clone(), dag.successors(id));
        pending.insert(id.clone(), dag.predecessors(id).len());
        let inc = dag
            .incoming_edges(id)
            .into_iter()
            .enumerate()
            .map(|(i, e)| {
                let port = e
                    .port
                    .clone()
                    .unwrap_or_else(|| if i == 0 { "src".to_string() } else { format!("src_{}", i + 1) });
                (e.from.clone(), port)
            })
            .collect();
        incoming.insert(id.clone(), inc);
    }

    // Runtime state.
    let mut statuses: HashMap<NodeId, RuntimeStatus> = HashMap::new();
    for id in &all_ids {
        statuses.insert(id.clone(), RuntimeStatus::Pending);
    }
    let mut outputs: HashMap<NodeId, Arc<Vec<DataFrame>>> = HashMap::new();
    let mut errors: HashMap<NodeId, DagError> = HashMap::new();

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
            if statuses[&id] != RuntimeStatus::Pending {
                // Already skipped/finished by a cascade — don't dispatch.
                continue;
            }
            let inputs = build_inputs(&id, &incoming, &outputs);
            let Some(mut node) = dag.take_node(&id) else {
                // Already taken (shouldn't happen for a Pending node).
                warn!(node = %id, "scheduler: node payload missing");
                continue;
            };
            statuses.insert(id.clone(), RuntimeStatus::Running);
            in_flight += 1;
            let tx = tx.clone();
            let sem = sem.clone();
            let job_id = id.clone();
            tokio::spawn(async move {
                let _permit = sem.acquire().await.ok();
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
                        warn!(node = %job_id, kind = error.kind(), "node failed");
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
            // All senders dropped unexpectedly.
            return Err(DagError::Schedule("result channel closed unexpectedly".into()));
        };
        in_flight -= 1;

        match msg {
            JobResult::Success { id, outputs: outs } => {
                let outs = Arc::new(outs);
                outputs.insert(id.clone(), outs);
                statuses.insert(id.clone(), RuntimeStatus::Success);
                debug!(node = %id, "node succeeded");
                for succ in &successors[&id] {
                    let left = {
                        let c = pending.entry(succ.clone()).or_insert(0);
                        *c = c.saturating_sub(1);
                        *c
                    };
                    if left == 0 && statuses[succ] == RuntimeStatus::Pending {
                        ready.push_back(succ.clone());
                    }
                }
            }
            JobResult::Failed { id, error } => {
                statuses.insert(id.clone(), RuntimeStatus::Failed);
                debug!(node = %id, kind = error.kind(), "node failed; cascading skip to descendants");
                errors.insert(id.clone(), error);
                cascade_skip(&id, &successors, &mut statuses, &mut ready);
            }
        }
    }

    let ok = !statuses
        .values()
        .any(|s| matches!(s, RuntimeStatus::Failed));

    Ok(RunReport {
        statuses,
        errors,
        ok,
    })
}

/// Gather a node's predecessor outputs into [`NodeInput`]s, in declared edge
/// order, cloning the [`DataFrame`] handles (cheap — they are `Arc` internally).
fn build_inputs(
    id: &str,
    incoming: &HashMap<NodeId, Vec<(NodeId, String)>>,
    outputs: &HashMap<NodeId, Arc<Vec<DataFrame>>>,
) -> Vec<NodeInput> {
    let mut inputs = Vec::new();
    if let Some(edges) = incoming.get(id) {
        for (from, port) in edges {
            if let Some(pred_outputs) = outputs.get(from) {
                for df in pred_outputs.iter() {
                    inputs.push(NodeInput {
                        port: port.clone(),
                        data: df.clone(),
                    });
                }
            }
        }
    }
    inputs
}

/// Mark every transitive descendant of `failed` as [`RuntimeStatus::Skipped`].
/// Stops at nodes that already have a terminal status so independent branches
/// keep running.
fn cascade_skip(
    failed: &str,
    successors: &HashMap<NodeId, Vec<NodeId>>,
    statuses: &mut HashMap<NodeId, RuntimeStatus>,
    ready: &mut VecDeque<NodeId>,
) {
    let mut queue: VecDeque<NodeId> = successors
        .get(failed)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .collect();
    while let Some(id) = queue.pop_front() {
        if statuses[&id] != RuntimeStatus::Pending {
            continue;
        }
        statuses.insert(id.clone(), RuntimeStatus::Skipped);
        ready.retain(|r| r != &id);
        if let Some(succs) = successors.get(&id) {
            for s in succs {
                queue.push_back(s.clone());
            }
        }
    }
}
