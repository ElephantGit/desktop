//! Production-path persistence smoke tests for Variable Aggregator V1.

use super::test_fixture::{ClockAt, NoopExecutor, SeqGen, bootstrap, seeded_pending_run};
use ora_application::{
    WorkflowGraph, WorkflowRepository, WorkflowRunEngine, WorkflowRunPayload, WorkflowRunRepository,
};
use ora_db::{
    SqliteWorkflowRepository, SqliteWorkflowRunEngineRepository, SqliteWorkflowRunRepository,
};
use ora_domain::{WorkflowId, WorkflowNodeStatus, WorkflowRunStatus, WorkflowSnapshotId};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::path::Path;
use std::process::Command;

/// Generates the persisted graph through the frontend's production graph serializer.
fn frontend_graph(smoke_case: &str) -> String {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("packages")
        .join("workflow-runtime")
        .join("tests")
        .join("aggregator-smoke-graph.ts");
    let script_path = script.to_string_lossy().replace('\\', "/");
    let script_url = if cfg!(windows) {
        format!("file:///{script_path}")
    } else {
        format!("file://{script_path}")
    };
    let program = format!(
        "import {{ serializeAggregatorSmokeGraph }} from '{script_url}'; console.log(serializeAggregatorSmokeGraph('{smoke_case}'));"
    );
    let output = Command::new("deno")
        .arg("eval")
        .arg(program)
        .output()
        .expect("run frontend workflow graph codec");
    assert!(
        output.status.success(),
        "frontend workflow graph codec failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("frontend graph is utf-8")
        .trim()
        .to_string()
}

/// Persists, reloads, parses, and executes one branch case through production repositories.
fn run_branch_case(
    smoke_case: &str,
    active_node: &str,
    inactive_node: &str,
    active_output: &str,
    expected_aggregator_output: Value,
    expected_downstream_output: Value,
    expected_selectors: &[&str],
) {
    let graph = frontend_graph(smoke_case);
    let (temp, pool) = bootstrap();
    let run_id = seeded_pending_run(&temp, &pool, &graph);

    let workflow_repository = SqliteWorkflowRepository::new(pool.clone());
    let reloaded = workflow_repository
        .find_snapshot_by_id(
            &WorkflowId::new("workflow-1"),
            &WorkflowSnapshotId::new("snapshot-1"),
        )
        .unwrap()
        .expect("persisted workflow snapshot");
    let reloaded_graph = WorkflowGraph::parse(&reloaded.graph).expect("parse reloaded graph");
    let selectors: Vec<String> = reloaded_graph
        .node("aggregator")
        .and_then(|node| node.aggregator_config.as_ref())
        .expect("reloaded aggregator config")
        .variables
        .iter()
        .map(|selector| selector.qualified())
        .collect();
    assert_eq!(
        selectors,
        expected_selectors
            .iter()
            .map(|selector| (*selector).to_string())
            .collect::<Vec<_>>()
    );

    let engine = WorkflowRunEngine::new(
        SqliteWorkflowRunEngineRepository::new(pool.clone()),
        NoopExecutor,
        SeqGen::default(),
        ClockAt(40),
    );
    engine.start(&run_id).unwrap();
    let run_repository = SqliteWorkflowRunRepository::new(pool.clone());
    let active = run_repository
        .list_node_runs(&run_id)
        .unwrap()
        .into_iter()
        .find(|node| node.node_id == active_node)
        .expect("active branch node run");
    assert_eq!(active.status, WorkflowNodeStatus::Running);
    assert!(
        run_repository
            .list_node_runs(&run_id)
            .unwrap()
            .iter()
            .all(|node| node.node_id != inactive_node),
        "inactive branch must not create a node run"
    );

    engine
        .complete_node(
            &run_id,
            &active.id,
            Some(active_output.to_string()),
            None,
            None,
            Vec::new(),
        )
        .unwrap();

    let run = run_repository.find_run(&run_id).unwrap().unwrap();
    assert_eq!(run.status, WorkflowRunStatus::Succeeded);
    assert_eq!(
        run.output.as_deref(),
        Some(expected_downstream_output.to_string().as_str())
    );
    let payload: WorkflowRunPayload =
        serde_json::from_str(run.payload.as_deref().expect("run payload")).unwrap();
    assert_eq!(
        payload.variable_pool.get("aggregator.output").unwrap(),
        Some(&expected_aggregator_output)
    );
    let node_runs = run_repository.list_node_runs(&run_id).unwrap();
    let downstream = node_runs
        .iter()
        .find(|node| node.node_id == "downstream")
        .expect("downstream node run");
    assert_eq!(
        downstream.output.as_deref(),
        Some(expected_downstream_output.to_string().as_str())
    );
}

/// A and B branch outputs survive frontend encoding, SQLite reload, and backend execution.
#[test]
fn persisted_branch_outputs_reach_downstream() {
    run_branch_case(
        "a",
        "a",
        "b",
        "from-a",
        json!("from-a"),
        json!({"result": "from-a"}),
        &["a.output", "b.output"],
    );
    run_branch_case(
        "b",
        "b",
        "a",
        "from-b",
        json!("from-b"),
        json!({"result": "from-b"}),
        &["a.output", "b.output"],
    );
}

/// A persisted boolean false remains assigned and reaches downstream unchanged.
#[test]
fn persisted_false_value_reaches_downstream() {
    run_branch_case(
        "false",
        "a",
        "b",
        "ignored-agent-output",
        json!(false),
        json!({"result": false}),
        &["flag.value"],
    );
}
