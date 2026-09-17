//! Graph-structure helpers for composite-region scheduling.
//!
//! These judgments live outside `engine.rs` so the scheduling core stays free of extra
//! region-bookkeeping bulk; they still answer from persisted rows and graph topology only.

use crate::workflow_run::engine::graph::WorkflowGraph;
use crate::workflow_run::engine::ports::FailurePropagation;
use ora_domain::WorkflowNodeRun;

/// Resolves how a failure of the node with the given id propagates, structurally: any failure
/// inside a composite region resolves to the owning composite node with `Composite` semantics
/// (the row fails, the run stays), and the owner's error strategy decides the node's fate at
/// settlement — `fail` fails the node and the run there, `continue` records the round and
/// advances (ADR "iteration composite runtime" D4, D6). This is a graph-structure judgment,
/// not a node-type branch in the scheduling core.
pub(super) fn region_failure_propagation(
    graph: &WorkflowGraph,
    node_id: &str,
) -> FailurePropagation {
    match graph.region_owner(node_id) {
        Some(_) => FailurePropagation::Composite,
        None => FailurePropagation::Run,
    }
}

/// Derives the round a composite node's region is currently executing, from the region's
/// persisted rows only (v1 serial execution; ADR "iteration composite runtime" D2).
pub(super) fn region_rows_round(
    graph: &WorkflowGraph,
    node_run: &WorkflowNodeRun,
    node_runs: &[WorkflowNodeRun],
) -> Option<u32> {
    let region = graph.region(&node_run.node_id)?;
    node_runs
        .iter()
        .filter(|row| row.iteration.is_some() && region.contains(&row.node_id))
        .filter_map(|row| row.iteration)
        .max()
}
