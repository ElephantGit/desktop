//! End-to-end coverage for iteration dispatch, round bindings, and swift failures.

use super::{
    current_thread_runtime, install_fake_opencode_plugin, open_ready_backend, seed_workspace,
};
use crate::setup::DesktopTestSetup;
use ora_contracts::*;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::time::Duration;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Keeps the production graph fixed while varying the prompt and iteration failure policy.
fn graph(prompt: &str, strategy: &str, ceiling: u32) -> Value {
    json!({"nodes": [
        {"id":"start","data":{"kind":"start","inputVariables":[
            {"name":"items","valueType":"array[string]"},
            {"name":"unset","valueType":"string"}
        ]}},
        {"id":"iter","data":{"kind":"iteration","iterationConfig":{
            "iteratorSelector":["start","items"],"collectSelector":["body","output"],
            "errorStrategy":strategy,"maxIterations":ceiling
        }}},
        {"id":"body","parentId":"iter","data":{"kind":"agent","agentConfig":{
            "executor":{"agentCli":"official/ora-space.opencode","modelId":"anthropic/claude-sonnet-4"},
            "prompt":prompt,"interactive":false
        }}},
        {"id":"out","data":{"kind":"output","outputs":[
            {"name":"collected","variableSelector":["iter","output"]}
        ]}}
    ],"edges":[
        {"source":"start","target":"iter"},
        {"source":"iter","target":"body"},
        {"source":"iter","target":"out"}
    ]})
}

/// Adds a condition whose comparison reads a declared but unassigned Start value.
fn failing_condition_graph(strategy: &str) -> Value {
    let mut graph = graph("hello", strategy, 10);
    graph["nodes"].as_array_mut().unwrap().push(json!({
        "id":"gate","parentId":"iter","data":{"kind":"condition","cases":[
            {"id":"yes","logic":"and","conditions":[
                {"variableSelector":["start","unset"],"operator":"equals","value":"x"}
            ]}
        ]}
    }));
    graph["edges"][1]["target"] = json!("gate");
    graph["edges"].as_array_mut().unwrap().push(json!({
        "source":"gate","sourceHandle":"yes","target":"body"
    }));
    graph
}

/// Runs one graph through the real Backend and fake ACP plugin, then checks its terminal state.
fn run_case(
    graph: Value,
    items: Value,
    expected: WorkflowRunStatus,
    sessions: usize,
    expected_output_fragment: Option<&str>,
) -> TestResult {
    ora_logging::with_trace_logging(|| {
        current_thread_runtime()?.block_on(async {
            let setup = DesktopTestSetup::new()?;
            let package = install_fake_opencode_plugin(&setup.backend_paths().home_directory)?;
            let backend = open_ready_backend(&setup)?;
            let workspace_id = seed_workspace(&setup, &backend)?;
            let workflow = backend
                .workflows()
                .create(CreateWorkflowRequest {
                    name: "Iteration E2E".into(),
                    graph: Some(graph.to_string()),
                })?
                .workflow;
            backend.workflows().publish(PublishWorkflowRequest {
                workflow_id: workflow.id.clone(),
                version: Some("v1".into()),
            })?;
            let run = backend
                .workflow_runs()
                .create(CreateWorkflowRunRequest {
                    workspace_id,
                    workflow_id: workflow.id,
                    locale: WorkflowRunLocale::EnUs,
                    snapshot_id: None,
                    kickoff_input: None,
                    name: None,
                })?
                .run;
            backend
                .workflow_runs()
                .update_input(UpdateWorkflowRunInputRequest {
                    run_id: run.id.clone(),
                    input: None,
                    variables: BTreeMap::from([("items".into(), items)]),
                })?;
            backend.workflow_runs().start(StartWorkflowRunRequest {
                run_id: run.id.clone(),
            })?;

            let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
            let mut observed = loop {
                let detail = backend.workflow_runs().get(GetWorkflowRunRequest {
                    run_id: run.id.clone(),
                })?;
                if matches!(
                    detail.run.status,
                    WorkflowRunStatus::Succeeded | WorkflowRunStatus::Failed
                ) || tokio::time::Instant::now() >= deadline
                {
                    break detail;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            };
            if observed.run.status == WorkflowRunStatus::Running {
                let cancelled = backend
                    .workflow_runs()
                    .cancel(CancelWorkflowRunRequest {
                        run_id: run.id.clone(),
                    })
                    .await?;
                observed = backend.workflow_runs().get(GetWorkflowRunRequest {
                    run_id: run.id.clone(),
                })?;
                assert_eq!(cancelled.run.status, WorkflowRunStatus::Cancelled);
            }
            let journal =
                std::fs::read_to_string(package.join("acp_calls.txt")).unwrap_or_default();
            let session_count = observed
                .nodes
                .iter()
                .filter(|node| node.session_id.is_some())
                .count();
            assert_eq!(observed.run.status, expected, "ACP journal: {journal}");
            assert_eq!(session_count, sessions, "ACP journal: {journal}");
            if let Some(fragment) = expected_output_fragment {
                assert!(
                    observed.nodes.iter().any(|node| {
                        node.output
                            .as_deref()
                            .is_some_and(|output| output.contains(fragment))
                    }),
                    "no node output contained {fragment:?}; ACP journal: {journal}",
                );
            }
            Ok(())
        })
    })
}

/// Every committed round gets a real session and the run reaches the terminal output node.
#[test]
fn second_round_really_dispatches() -> TestResult {
    run_case(
        graph("hello", "fail", 10),
        json!(["a", "b"]),
        WorkflowRunStatus::Succeeded,
        2,
        None,
    )
}

/// Round-start bindings are committed before the first prompt is rendered.
#[test]
fn round_prompt_reads_item_and_index() -> TestResult {
    run_case(
        graph("item={{#iter.item#}} index={{#iter.index#}}", "fail", 10),
        json!(["a"]),
        WorkflowRunStatus::Succeeded,
        1,
        Some("item=a index=0"),
    )
}

/// A swift failure settles a fail-strategy iteration even when no async callback is pending.
#[test]
fn failed_condition_fails_the_iteration() -> TestResult {
    run_case(
        failing_condition_graph("fail"),
        json!(["a"]),
        WorkflowRunStatus::Failed,
        0,
        None,
    )
}

/// Continue absorbs that swift failure and completes the run without opening an Agent session.
#[test]
fn failed_condition_continue_completes_the_iteration() -> TestResult {
    run_case(
        failing_condition_graph("continue"),
        json!(["a"]),
        WorkflowRunStatus::Succeeded,
        0,
        None,
    )
}
