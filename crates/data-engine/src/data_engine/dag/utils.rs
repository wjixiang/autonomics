use std::collections::VecDeque;

use datafusion::common::HashMap;

use crate::data_engine::dag::{NodeId, NodeInput, RuntimeStatus, graph::{EdgeLabel, NamedDataFrames}};

/// Gather a node's predecessor outputs into [`NodeInput`]s, in declared edge
/// order, cloning the [`DataFrame`] handles (cheap — they are `Arc` internally).
///
/// Each input's `df_name` is taken from the edge's port label (not the
/// upstream node's `output_df_name`), ensuring globally-unique table names
/// in the shared `SessionContext`.
pub fn build_inputs(
    id: &str,
    incoming: &HashMap<NodeId, Vec<(NodeId, EdgeLabel)>>,
    outputs: &HashMap<NodeId, NamedDataFrames>,
) -> Vec<NodeInput> {
    let mut inputs = Vec::new();
    if let Some(edges) = incoming.get(id) {
        for (from, edge) in edges {
            if let Some(pred_outputs) = outputs.get(from) {
                for (_df_name, df) in pred_outputs.iter() {
                    inputs.push(NodeInput {
                        df_name: edge.port.clone(),
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
pub fn cascade_skip(
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
