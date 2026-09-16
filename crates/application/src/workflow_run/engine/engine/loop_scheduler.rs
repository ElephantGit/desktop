use super::{EngineError, LoopScheduleOutcome, NodeExecutor, WorkflowRunEngine};
use crate::project::Clock;
use crate::workflow_run::engine::branch_projection::BranchProjection;
use crate::workflow_run::engine::graph::WorkflowGraph;
use crate::workflow_run::engine::node_type::NodeType;
use crate::workflow_run::engine::ports::{
    ExecutionContext, LoopRoundAdvance, NodeRunToStart, WorkflowNodeRunIdGenerator,
    WorkflowRunEngineRepository,
};
use crate::workflow_run::engine::variable_pool::WorkflowVariablePool;
use crate::workflow_run::engine::{LoopRoundDecision, LoopRoundExecutionState};
use ora_domain::{WorkflowNodeRun, WorkflowRunId};

/// Runs one isolated scheduling pass for a Loop parent and its active round.
pub(super) fn run_loop_schedule<R, E, G, C>(
    engine: &WorkflowRunEngine<R, E, G, C>,
    run_id: &WorkflowRunId,
    context: &ExecutionContext,
    graph: &WorkflowGraph,
    loop_node_run: &WorkflowNodeRun,
    outer_pool: &WorkflowVariablePool,
    now: i64,
) -> Result<LoopScheduleOutcome, EngineError>
where
    R: WorkflowRunEngineRepository,
    E: NodeExecutor,
    G: WorkflowNodeRunIdGenerator,
    C: Clock,
{
    let Some((config, body)) = graph.loop_body(&loop_node_run.node_id) else {
        engine.repository.fail_node(
            &loop_node_run.id,
            format!("Loop {} has no executable body", loop_node_run.node_id),
            None,
            now,
        )?;
        return Ok(LoopScheduleOutcome::Progressed);
    };
    let Some(scope) = engine
        .repository
        .find_active_loop_round(&loop_node_run.id)?
    else {
        let carried = match config.initialize_carried(outer_pool) {
            Ok(carried) => carried,
            Err(error) => {
                engine
                    .repository
                    .fail_node(&loop_node_run.id, error.to_string(), None, now)?;
                return Ok(LoopScheduleOutcome::Progressed);
            }
        };
        let round_pool = match graph.loop_round_pool(&loop_node_run.node_id, outer_pool, &carried) {
            Ok(pool) => pool,
            Err(error) => {
                engine
                    .repository
                    .fail_node(&loop_node_run.id, error.to_string(), None, now)?;
                return Ok(LoopScheduleOutcome::Progressed);
            }
        };
        let round = engine.prepare_loop_round(loop_node_run, body, 1, round_pool)?;
        engine.repository.start_loop_round(run_id, &round, now)?;
        return Ok(LoopScheduleOutcome::Progressed);
    };

    let state = match serde_json::from_str::<LoopRoundExecutionState>(&scope.state) {
        Ok(state) => state,
        Err(error) => {
            engine.repository.advance_loop_round(
                &scope.id,
                &LoopRoundAdvance::Fail {
                    error: format!("Loop round state is invalid: {error}"),
                },
                now,
            )?;
            return Ok(LoopScheduleOutcome::Progressed);
        }
    };
    let node_runs = engine.repository.list_node_runs_in_scope(&scope.id)?;
    if engine.complete_loop_controls(&scope, body, context, &state, &node_runs, now)? {
        return Ok(LoopScheduleOutcome::Progressed);
    }

    let node_runs = engine.repository.list_node_runs_in_scope(&scope.id)?;
    let projection = BranchProjection::new(body, &node_runs, &state.condition_decisions);
    let ready = projection.ready_nodes();
    if !ready.is_empty() {
        let ready_runs: Vec<NodeRunToStart> = ready
            .iter()
            .map(|node| NodeRunToStart {
                id: engine.node_run_id_generator.generate_node_run_id(),
                scope_id: scope.id.clone(),
                node_id: node.id.clone(),
                node_type: node.node_type.as_str().to_string(),
                input: None,
            })
            .collect();
        engine
            .repository
            .start_scope_ready_nodes(&scope.id, &ready_runs, now)?;
        for (node, node_run) in ready.iter().zip(&ready_runs) {
            if node.node_type == NodeType::Agent {
                engine.node_executor.dispatch(
                    &node_run.id,
                    node,
                    body,
                    context,
                    &scope.id,
                    &state.variable_pool,
                );
            }
        }
        return Ok(LoopScheduleOutcome::Progressed);
    }
    if projection.has_in_flight() {
        return Ok(LoopScheduleOutcome::Waiting);
    }

    let advance = match config.complete_round(scope.round_index, &state.variable_pool) {
        Ok(LoopRoundDecision::Continue { carried }) => {
            let next_pool =
                match graph.loop_round_pool(&loop_node_run.node_id, outer_pool, &carried) {
                    Ok(pool) => pool,
                    Err(error) => {
                        return engine.fail_loop_round(&scope, error.to_string(), now);
                    }
                };
            LoopRoundAdvance::Continue {
                next: engine.prepare_loop_round(
                    loop_node_run,
                    body,
                    scope.round_index + 1,
                    next_pool,
                )?,
            }
        }
        Ok(LoopRoundDecision::Succeeded { outputs }) => LoopRoundAdvance::Succeed { outputs },
        Err(error) => return engine.fail_loop_round(&scope, error.to_string(), now),
    };
    engine
        .repository
        .advance_loop_round(&scope.id, &advance, now)?;
    Ok(LoopScheduleOutcome::Progressed)
}
