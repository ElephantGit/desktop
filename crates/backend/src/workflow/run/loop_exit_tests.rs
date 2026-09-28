//! Explicit breaks exercise real SQLite transactions, callback fences, and restart replay.
use super::test_fixture::{ClockAt, NoopExecutor, bootstrap, started_run_with};
use ora_application::{
    ExecutionContext, LoopExitCleanup, NodeExecutor, UuidWorkflowNodeRunIdGenerator, WorkflowGraph,
    WorkflowGraphNode, WorkflowRunEngine, WorkflowRunEngineRepository, WorkflowRunRepository,
    WorkflowVariablePool,
};
use ora_db::{SqliteWorkflowRunEngineRepository, SqliteWorkflowRunRepository};
use ora_domain::{WorkflowNodeRunId, WorkflowNodeStatus, WorkflowRunStatus, WorkflowScopeId};
use ora_logging::with_trace_logging;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};

/// Builds a conditional break with an intentionally unavailable next-round feedback source.
pub(super) fn graph(parallel: bool) -> String {
    let agent = |id: &str| json!({"id":id,"data":{"kind":"agent","containerId":"loop","agentConfig":{"executor":{"agentCli":"c","modelId":"m"},"prompt":"work"}}});
    let mut graph = json!({"schemaVersion":2,"nodes":[
        {"id":"start","data":{"kind":"start"}},
        {"id":"loop","data":{"kind":"loop","loopConfig":{
            "maxIterations":2,"variables":[{"name":"value","valueType":"string","initial":{"kind":"constant","value":"seed"},"feedback":["again","output"]}],
            "until":{"logic":"and","conditions":[]},"outputs":[{"name":"result","variableSelector":["writer","output"]}]
        }}},
        {"id":"out","data":{"kind":"output","outputs":[{"name":"result","variableSelector":["loop","result"]}]}},
        {"id":"entry","data":{"kind":"start","containerId":"loop"}}, agent("writer"), agent("again"),
        {"id":"check","data":{"kind":"condition","containerId":"loop","cases":[{"id":"done","logic":"and","conditions":[{"variableSelector":["writer","output"],"operator":"equals","value":"done"}]}]}},
        {"id":"exit","data":{"kind":"loopExit","containerId":"loop"}}
    ],"edges":[
        {"source":"start","target":"loop"},{"source":"loop","target":"out"},
        {"source":"entry","target":"writer"},{"source":"writer","target":"check"},
        {"source":"check","sourceHandle":"done","target":"exit"},
        {"source":"check","sourceHandle":"else","target":"again"}
    ]});
    if parallel {
        graph["nodes"].as_array_mut().unwrap().push(agent("slow"));
        graph["edges"]
            .as_array_mut()
            .unwrap()
            .push(json!({"source":"entry","target":"slow"}));
    }
    graph.to_string()
}

/// A controllable cleanup acknowledgement models a real asynchronous session owner.
pub(super) struct PendingCleanup;
impl NodeExecutor for PendingCleanup {
    /// Dispatch stays under test control.
    fn dispatch(
        &self,
        _: &WorkflowNodeRunId,
        _: &WorkflowGraphNode,
        _: &WorkflowGraph,
        _: &ExecutionContext,
        _: &WorkflowScopeId,
        _: &WorkflowVariablePool,
    ) {
    }
    /// Keeps the parent running until the test acknowledges scoped cleanup.
    fn cleanup_loop_exit(
        &self,
        _: &ExecutionContext,
        _: &WorkflowNodeRunId,
        _: &WorkflowScopeId,
    ) -> LoopExitCleanup {
        LoopExitCleanup::Pending
    }
}

/// A reached break succeeds without reading the unexecuted feedback or starting another round.
#[test]
fn loop_exit_publishes_outputs_without_feedback() {
    with_trace_logging(|| {
        let (temp, pool) = bootstrap();
        let (id, nodes, engine) =
            started_run_with(&temp, &pool, &graph(/*parallel*/ false), NoopExecutor);
        let writer = nodes.iter().find(|node| node.node_id == "writer").unwrap();
        engine
            .complete_node(&id, &writer.id, Some("done".into()), None, None, vec![])
            .unwrap();
        let repo = SqliteWorkflowRunRepository::new(pool);
        let run = repo.find_run(&id).unwrap().unwrap();
        assert_eq!(
            (run.status, run.output),
            (
                WorkflowRunStatus::Succeeded,
                Some(r#"{"result":"done"}"#.into())
            )
        );
        let nodes = repo.list_node_runs(&id).unwrap();
        assert_eq!(
            nodes.iter().filter(|node| node.node_id == "writer").count(),
            1
        );
        assert!(!nodes.iter().any(|node| node.node_id == "again"));
        let parent = nodes.iter().find(|node| node.node_id == "loop").unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(parent.payload.as_deref().unwrap()).unwrap()["stop_reason"],
            "loop_exit"
        );
    });
}

/// Exit fences siblings before cleanup and survives a crash before its acknowledgement.
#[test]
fn loop_exit_recovers_pending_cleanup_and_rejects_late_completion() {
    with_trace_logging(|| {
        let (temp, pool) = bootstrap();
        let (id, nodes, engine) =
            started_run_with(&temp, &pool, &graph(/*parallel*/ true), PendingCleanup);
        let writer = nodes.iter().find(|node| node.node_id == "writer").unwrap();
        let slow = nodes.iter().find(|node| node.node_id == "slow").unwrap();
        let parent = nodes.iter().find(|node| node.node_id == "loop").unwrap();
        engine
            .complete_node(&id, &writer.id, Some("done".into()), None, None, vec![])
            .unwrap();
        let repo = SqliteWorkflowRunEngineRepository::new(pool.clone());
        let scope = repo.find_active_loop_round(&parent.id).unwrap().unwrap();
        assert!(scope.state.contains("requestedAt"));
        engine
            .complete_node(&id, &slow.id, Some("late".into()), None, None, vec![])
            .unwrap();
        let after = repo.list_node_runs(&id).unwrap();
        assert_eq!(
            after.iter().find(|node| node.id == slow.id).unwrap().status,
            WorkflowNodeStatus::Cancelled
        );
        assert!(!after.iter().any(|node| node.node_id == "out"));
        // A fresh engine has no in-memory break state. The persisted request remains authoritative.
        let recovered = WorkflowRunEngine::new(
            repo.clone(),
            PendingCleanup,
            UuidWorkflowNodeRunIdGenerator,
            ClockAt(70),
        );
        super::recovery::sweep_one_run(&repo, &id, 65).unwrap();
        recovered.resume(&id).unwrap();
        recovered
            .finish_loop_exit(&id, &parent.id, &scope.id, Ok(()))
            .unwrap();
        recovered
            .finish_loop_exit(&id, &parent.id, &scope.id, Ok(()))
            .unwrap();
        let run = SqliteWorkflowRunRepository::new(pool)
            .find_run(&id)
            .unwrap()
            .unwrap();
        assert_eq!(
            (run.status, run.output),
            (
                WorkflowRunStatus::Succeeded,
                Some(r#"{"result":"done"}"#.into())
            )
        );
    });
}

/// Graph validation rejects breaks outside loops and edges after a break, including imported graphs.
#[test]
fn loop_exit_requires_a_terminal_loop_member() {
    with_trace_logging(|| {
        let mut document: Value = serde_json::from_str(&graph(/*parallel*/ false)).unwrap();
        document["edges"]
            .as_array_mut()
            .unwrap()
            .push(json!({"source":"exit","target":"again"}));
        assert!(WorkflowGraph::parse(&document.to_string()).is_err());
        assert!(WorkflowGraph::parse(r#"{"nodes":[{"id":"s","data":{"kind":"start"}},{"id":"e","data":{"kind":"loopExit"}}],"edges":[{"source":"s","target":"e"}]}"#).is_err());
    });
}

/// Missing current-round output is explicit failure, never a previous-round fallback.
#[test]
fn loop_exit_rejects_unavailable_result() {
    with_trace_logging(|| {
        let (temp, pool) = bootstrap();
        let mut document: Value = serde_json::from_str(&graph(/*parallel*/ false)).unwrap();
        document["nodes"][1]["data"]["loopConfig"]["outputs"][0]["variableSelector"] =
            json!(["again", "output"]);
        let (id, nodes, engine) =
            started_run_with(&temp, &pool, &document.to_string(), NoopExecutor);
        let writer = nodes.iter().find(|node| node.node_id == "writer").unwrap();
        engine
            .complete_node(&id, &writer.id, Some("done".into()), None, None, vec![])
            .unwrap();
        let run = SqliteWorkflowRunRepository::new(pool)
            .find_run(&id)
            .unwrap()
            .unwrap();
        assert_eq!(run.status, WorkflowRunStatus::Failed);
        assert!(run.error.unwrap().contains("again.output"));
    });
}

/// A break in an unselected branch does not stop feedback or the next round.
#[test]
fn loop_exit_only_fires_on_the_taken_branch() {
    with_trace_logging(|| {
        let (temp, pool) = bootstrap();
        let (id, nodes, engine) =
            started_run_with(&temp, &pool, &graph(/*parallel*/ false), NoopExecutor);
        let writer = nodes.iter().find(|node| node.node_id == "writer").unwrap();
        engine
            .complete_node(&id, &writer.id, Some("continue".into()), None, None, vec![])
            .unwrap();
        let repo = SqliteWorkflowRunEngineRepository::new(pool.clone());
        let nodes = repo.list_node_runs(&id).unwrap();
        assert!(!nodes.iter().any(|node| node.node_id == "exit"));
        let again = nodes.iter().find(|node| node.node_id == "again").unwrap();
        engine
            .complete_node(&id, &again.id, Some("next".into()), None, None, vec![])
            .unwrap();
        let nodes = repo.list_node_runs(&id).unwrap();
        let next = nodes
            .iter()
            .find(|node| node.node_id == "writer" && node.status == WorkflowNodeStatus::Running)
            .unwrap();
        assert_ne!(next.scope_id, writer.scope_id);
        engine
            .complete_node(&id, &next.id, Some("done".into()), None, None, vec![])
            .unwrap();
        assert_eq!(
            SqliteWorkflowRunRepository::new(pool)
                .find_run(&id)
                .unwrap()
                .unwrap()
                .status,
            WorkflowRunStatus::Succeeded
        );
    });
}

/// User cancellation wins over an acknowledgement that arrives after the run was cancelled.
#[test]
fn loop_exit_cleanup_cannot_revive_a_cancelled_run() {
    with_trace_logging(|| {
        let (temp, pool) = bootstrap();
        let (id, nodes, engine) =
            started_run_with(&temp, &pool, &graph(/*parallel*/ true), PendingCleanup);
        let writer = nodes.iter().find(|node| node.node_id == "writer").unwrap();
        let parent = nodes.iter().find(|node| node.node_id == "loop").unwrap();
        engine
            .complete_node(&id, &writer.id, Some("done".into()), None, None, vec![])
            .unwrap();
        engine.cancel(&id).unwrap();
        engine
            .finish_loop_exit(&id, &parent.id, &writer.scope_id, Ok(()))
            .unwrap();
        let repo = SqliteWorkflowRunRepository::new(pool);
        assert_eq!(
            repo.find_run(&id).unwrap().unwrap().status,
            WorkflowRunStatus::Cancelled
        );
        assert!(
            !repo
                .list_node_runs(&id)
                .unwrap()
                .iter()
                .any(|node| node.node_id == "out")
        );
    });
}

/// Cleanup failure cannot publish a successful result or dispatch outer successors.
#[test]
fn loop_exit_cleanup_failure_fails_closed() {
    with_trace_logging(|| {
        let (temp, pool) = bootstrap();
        let (id, nodes, engine) =
            started_run_with(&temp, &pool, &graph(/*parallel*/ true), PendingCleanup);
        let writer = nodes.iter().find(|node| node.node_id == "writer").unwrap();
        let parent = nodes.iter().find(|node| node.node_id == "loop").unwrap();
        engine
            .complete_node(
                &id,
                &writer.id,
                Some("done".into()),
                /*structured_output*/ None,
                /*stop_reason*/ None,
                vec![],
            )
            .unwrap();
        engine
            .finish_loop_exit(
                &id,
                &parent.id,
                &writer.scope_id,
                Err("session stop failed".into()),
            )
            .unwrap();
        let repo = SqliteWorkflowRunRepository::new(pool);
        let run = repo.find_run(&id).unwrap().unwrap();
        assert_eq!(
            (run.status, run.error),
            (
                WorkflowRunStatus::Failed,
                Some("session stop failed".into())
            )
        );
        assert!(
            !repo
                .list_node_runs(&id)
                .unwrap()
                .iter()
                .any(|node| node.node_id == "out")
        );
    });
}

/// A break cancels only its own round; concurrently executing outer work survives.
#[test]
fn loop_exit_preserves_outer_parallel_work() {
    with_trace_logging(|| {
        let (temp, pool) = bootstrap();
        let mut document: Value = serde_json::from_str(&graph(/*parallel*/ true)).unwrap();
        document["nodes"].as_array_mut().unwrap().push(json!({"id":"outside","data":{"kind":"agent","agentConfig":{"executor":{"agentCli":"c","modelId":"m"},"prompt":"outside work"}}}));
        document["edges"]
            .as_array_mut()
            .unwrap()
            .push(json!({"source":"start","target":"outside"}));
        let (id, nodes, engine) =
            started_run_with(&temp, &pool, &document.to_string(), NoopExecutor);
        let writer = nodes.iter().find(|node| node.node_id == "writer").unwrap();
        engine
            .complete_node(
                &id,
                &writer.id,
                Some("done".into()),
                /*structured_output*/ None,
                /*stop_reason*/ None,
                vec![],
            )
            .unwrap();
        let after = SqliteWorkflowRunRepository::new(pool)
            .list_node_runs(&id)
            .unwrap();
        assert_eq!(
            after
                .iter()
                .filter(|node| ["outside", "slow"].contains(&node.node_id.as_str()))
                .map(|node| (node.node_id.as_str(), node.status))
                .collect::<std::collections::BTreeMap<_, _>>(),
            std::collections::BTreeMap::from([
                ("outside", WorkflowNodeStatus::Running),
                ("slow", WorkflowNodeStatus::Cancelled)
            ])
        );
    });
}
