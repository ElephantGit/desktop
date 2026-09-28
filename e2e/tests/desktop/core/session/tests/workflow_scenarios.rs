//! User-level workflow scenario coverage through the production Backend interface.
//!
//! Every scenario publishes an editor-shaped graph, supplies typed Start values the way the
//! run-start screen does, starts the run, and asserts the user-visible outcome: terminal
//! status, per-round agent sessions, rendered prompts, and the exposed variable pool. The
//! fake OpenCode agent echoes each rendered prompt back as its output, so prompt templates
//! double as data-flow assertions.

use super::{
    current_thread_runtime, install_fake_opencode_plugin, open_ready_backend, seed_workspace,
};
use crate::setup::DesktopTestSetup;
use ora_backend::Backend;
use ora_contracts::*;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// The agent CLI the fake plugin answers for; the model id is never validated by the fake.
const AGENT_CLI: &str = "official/ora-space.opencode";
/// Generous deadline: composite scenarios open one session per round through a real backend.
const RUN_DEADLINE: Duration = Duration::from_secs(20);

/// One executed run together with the fake-agent journal for failure diagnostics.
struct ScenarioRun {
    detail: GetWorkflowRunResponse,
    journal: String,
}

impl ScenarioRun {
    fn status(&self) -> WorkflowRunStatus {
        self.detail.run.status
    }

    /// Sessions opened for this run; one per executed agent round.
    fn sessions(&self) -> usize {
        self.detail
            .nodes
            .iter()
            .filter(|node| node.session_id.is_some())
            .count()
    }

    /// Outputs of every row recorded for one node id, in persisted order.
    fn node_outputs(&self, node_id: &str) -> Vec<&str> {
        self.detail
            .nodes
            .iter()
            .filter(|node| node.node_id == node_id)
            .filter_map(|node| node.output.as_deref())
            .collect()
    }

    /// Errors recorded on one node's rows.
    fn node_errors(&self, node_id: &str) -> Vec<&str> {
        self.detail
            .nodes
            .iter()
            .filter(|node| node.node_id == node_id)
            .filter_map(|node| node.error.as_deref())
            .collect()
    }

    /// Current value of one exposed root variable, e.g. (`iter`, `failed_count`).
    fn variable(&self, node_id: &str, root: &str) -> Option<&Value> {
        self.detail
            .variables
            .iter()
            .find(|variable| {
                variable
                    .selector
                    .first()
                    .is_some_and(|part| part == node_id)
                    && variable.selector.get(1).is_some_and(|part| part == root)
            })
            .and_then(|variable| variable.value.as_ref())
    }

    /// Node-level failure detail for assertion messages and CI diagnostics.
    fn failure_summary(&self) -> String {
        let nodes = self
            .detail
            .nodes
            .iter()
            .map(|node| {
                format!(
                    "{}({:?}{}{})",
                    node.node_id,
                    node.status,
                    node.error
                        .as_deref()
                        .map(|error| format!(", error: {error}"))
                        .unwrap_or_default(),
                    node.output
                        .as_deref()
                        .map(|output| format!(
                            ", output: {}",
                            output.chars().take(120).collect::<String>()
                        ))
                        .unwrap_or_default(),
                )
            })
            .collect::<Vec<_>>()
            .join(" | ");
        format!(
            "status {:?}; run error {:?}; nodes [{}]; journal {}",
            self.detail.run.status, self.detail.run.error, nodes, self.journal
        )
    }

    /// The run's terminal output document, parsed as JSON.
    fn run_output(&self) -> Value {
        serde_json::from_str(
            self.detail
                .run
                .output
                .as_deref()
                .unwrap_or_else(|| panic!("run has no output; journal: {}", self.journal)),
        )
        .unwrap_or_else(|error| panic!("run output is not JSON: {error}"))
    }
}

/// Drives published graphs through the same production handlers the desktop UI calls.
struct WorkflowHarness {
    backend: Backend,
    package_root: PathBuf,
    workspace_id: String,
    workflow_seq: usize,
    /// Kept alive so the sandbox's TempDir outlives the backend; dropping it first would delete
    /// the workspace directory out from under running sessions (Unix deletes eagerly).
    _setup: DesktopTestSetup,
}

impl WorkflowHarness {
    /// Opens a ready backend with the fake OpenCode agent installed.
    fn open() -> Result<Self, Box<dyn std::error::Error>> {
        let setup = DesktopTestSetup::new()?;
        let package_root = install_fake_opencode_plugin(&setup.backend_paths().home_directory)?;
        let backend = open_ready_backend(&setup)?;
        let workspace_id = seed_workspace(&setup, &backend)?;
        Ok(Self {
            backend,
            package_root,
            workspace_id,
            workflow_seq: 0,
            _setup: setup,
        })
    }

    /// Publishes the graph and drives one run to a terminal status with the given Start values.
    async fn run(
        &mut self,
        graph: Value,
        variables: BTreeMap<String, Value>,
    ) -> Result<ScenarioRun, Box<dyn std::error::Error>> {
        let run_id = self.pending_run(graph)?;
        self.run_on_pending(run_id, variables).await
    }

    /// Supplies values to an already-created pending run and drives it to a terminal status.
    async fn run_on_pending(
        &self,
        run_id: String,
        variables: BTreeMap<String, Value>,
    ) -> Result<ScenarioRun, Box<dyn std::error::Error>> {
        let result = self
            .backend
            .workflow_runs()
            .update_input(UpdateWorkflowRunInputRequest {
                run_id: run_id.clone(),
                input: None,
                variables,
            });
        result.map_err(|error| format!("update_input rejected valid values: {error}"))?;
        self.backend
            .workflow_runs()
            .start(StartWorkflowRunRequest {
                run_id: run_id.clone(),
            })?;
        self.wait_terminal(&run_id).await
    }

    /// Creates a pending run on a freshly published graph without providing values yet.
    fn pending_run(&mut self, graph: Value) -> Result<String, Box<dyn std::error::Error>> {
        self.workflow_seq += 1;
        let workflow = self
            .backend
            .workflows()
            .create(CreateWorkflowRequest {
                name: format!("Scenario {}", self.workflow_seq),
                graph: Some(graph.to_string()),
            })?
            .workflow;
        self.backend.workflows().publish(PublishWorkflowRequest {
            workflow_id: workflow.id.clone(),
            version: Some("v1".into()),
        })?;
        let run = self
            .backend
            .workflow_runs()
            .create(CreateWorkflowRunRequest {
                inject_last_failure: None,
                workspace_id: self.workspace_id.clone(),
                workflow_id: workflow.id,
                locale: WorkflowRunLocale::EnUs,
                snapshot_id: None,
                kickoff_input: None,
                name: None,
            })?
            .run;
        Ok(run.id)
    }

    /// Polls the most recently created run until it reaches a terminal status.
    async fn wait_terminal(&self, run_id: &str) -> Result<ScenarioRun, Box<dyn std::error::Error>> {
        let detail = self.poll(run_id).await?;
        let journal =
            std::fs::read_to_string(self.package_root.join("acp_calls.txt")).unwrap_or_default();
        Ok(ScenarioRun { detail, journal })
    }

    /// Yields with `tokio::time::sleep` so the current-thread runtime keeps driving the
    /// backend's spawned session pumps between polls.
    async fn poll(
        &self,
        run_id: &str,
    ) -> Result<GetWorkflowRunResponse, Box<dyn std::error::Error>> {
        let deadline = tokio::time::Instant::now() + RUN_DEADLINE;
        loop {
            let detail = self.backend.workflow_runs().get(GetWorkflowRunRequest {
                run_id: run_id.into(),
            })?;
            if matches!(
                detail.run.status,
                WorkflowRunStatus::Succeeded | WorkflowRunStatus::Failed
            ) {
                return Ok(detail);
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "run did not settle within {RUN_DEADLINE:?}; status {:?}; nodes {:?}",
                detail.run.status,
                detail
                    .nodes
                    .iter()
                    .map(|node| (node.node_id.clone(), node.status))
                    .collect::<Vec<_>>(),
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

/// Agent-node data with the fake OpenCode executor; the prompt is echoed back on output.
fn agent_data(prompt: &str) -> Value {
    json!({"kind":"agent","agentConfig":{
        "executor":{"agentCli":AGENT_CLI,"modelId":"anthropic/claude-sonnet-4"},
        "prompt":prompt,"interactive":false
    }})
}

/// Agent-node data additionally owned by a Loop container.
fn loop_agent_data(prompt: &str, loop_id: &str) -> Value {
    let mut data = agent_data(prompt);
    data["containerId"] = json!(loop_id);
    data
}

/// Start-input declaration with an explicit field control and pool type.
fn input(name: &str, field_type: &str, value_type: &str) -> Value {
    json!({"name":name,"fieldType":field_type,"valueType":value_type})
}

// ── Start inputs: every supported data type ──

/// Every supported Start field type supplies a matching value that reaches the agent prompt
/// rendered per its pool type: strings verbatim, everything else as JSON.
#[test]
fn start_values_of_every_supported_type_reach_the_agent_prompt() -> TestResult {
    ora_logging::with_trace_logging(|| {
        current_thread_runtime()?.block_on(async {
            let mut harness = WorkflowHarness::open()?;
            let graph = json!({
                "nodes": [
                    {"id":"start","type":"workflow","position":{"x":0,"y":0},"data":{"kind":"start","inputVariables":[
                        input("text","text-input","string"),
                        input("count","number","number"),
                        input("flag","checkbox","boolean"),
                        {"name":"choice","fieldType":"select","valueType":"string","options":["x","y"]},
                        input("tags","json","array[string]"),
                        input("rows","json","array[object]"),
                        input("meta","json","object"),
                        input("raw","json","any")
                    ]}},
                    {"id":"agent","type":"workflow","position":{"x":360,"y":0},"data":agent_data(
                        "text={{#start.text#}} count={{#start.count#}} flag={{#start.flag#}} choice={{#start.choice#}} tags={{#start.tags#}} rows={{#start.rows#}} meta={{#start.meta#}} raw={{#start.raw#}}"
                    )},
                    {"id":"out","type":"workflow","position":{"x":720,"y":0},"data":{"kind":"output","outputs":[
                        {"name":"echo","variableSelector":["agent","output"]}
                    ]}}
                ],
                "edges":[
                    {"source":"start","target":"agent"},
                    {"source":"agent","target":"out"}
                ]
            });
            let run = harness.run(graph, BTreeMap::from([
                ("text".into(), json!("hello")),
                ("count".into(), json!(7)),
                ("flag".into(), json!(true)),
                ("choice".into(), json!("x")),
                ("tags".into(), json!(["a","b"])),
                ("rows".into(), json!([{"id":1}])),
                ("meta".into(), json!({"k":"v"})),
                ("raw".into(), json!(42)),
            ])).await?;

            assert_eq!(run.status(), WorkflowRunStatus::Succeeded, "{}", run.failure_summary());
            assert_eq!(run.sessions(), 1, "{}", run.failure_summary());
            let output = run.node_outputs("agent").join("\n");
            for fragment in [
                "text=hello",
                "count=7",
                "flag=true",
                "choice=x",
                "tags=[\"a\",\"b\"]",
                "rows=[{\"id\":1}]",
                "meta={\"k\":\"v\"}",
                "raw=42",
            ] {
                assert!(output.contains(fragment), "missing {fragment:?} in {output:?}");
            }
            assert_eq!(run.variable("start", "text"), Some(&json!("hello")));
            assert_eq!(run.variable("start", "count"), Some(&json!(7)));
            assert_eq!(run.variable("start", "tags"), Some(&json!(["a","b"])));
            Ok(())
        })
    })
}

/// The run-input screen's writes are validated against the declared pool types before the run
/// starts; a value of the wrong shape never enters the pool.
#[test]
fn run_inputs_are_validated_against_declared_types() -> TestResult {
    ora_logging::with_trace_logging(|| {
        current_thread_runtime()?.block_on(async {
            let mut harness = WorkflowHarness::open()?;
            let graph = json!({
                "nodes": [
                    {"id":"start","type":"workflow","position":{"x":0,"y":0},"data":{"kind":"start","inputVariables":[
                        input("count","number","number"),
                        input("meta","json","object"),
                        input("tags","json","array[string]"),
                        input("required_text","text-input","string")
                    ]}},
                    {"id":"agent","type":"workflow","position":{"x":360,"y":0},"data":agent_data("ok")},
                    {"id":"out","type":"workflow","position":{"x":720,"y":0},"data":{"kind":"output","outputs":[
                        {"name":"echo","variableSelector":["agent","output"]}
                    ]}}
                ],
                "edges":[
                    {"source":"start","target":"agent"},
                    {"source":"agent","target":"out"}
                ]
            });
            let run_id = harness.pending_run(graph)?;

            let cases: [(String, Value); 4] = [
                ("count".into(), json!("not-a-number")),
                ("meta".into(), json!([1, 2])),
                ("tags".into(), json!({"a": 1})),
                ("typo".into(), json!("value")),
            ];
            for (name, value) in cases {
                let error = harness
                    .backend
                    .workflow_runs()
                    .update_input(UpdateWorkflowRunInputRequest {
                        run_id: run_id.clone(),
                        input: None,
                        variables: BTreeMap::from([(name.clone(), value)]),
                    })
                    .err()
                    .ok_or(format!("update_input accepted a mistyped value for {name}"))?;
                eprintln!("update_input rejection for {name}: {error}");
            }

            harness
                .backend
                .workflow_runs()
                .update_input(UpdateWorkflowRunInputRequest {
                    run_id: run_id.clone(),
                    input: None,
                    variables: BTreeMap::from([
                        ("count".into(), json!(3)),
                        ("meta".into(), json!({"k":"v"})),
                        ("tags".into(), json!(["a"])),
                        ("required_text".into(), json!("provided")),
                    ]),
                })
                .map_err(|error| format!("valid values were rejected: {error}"))?;
            harness
                .backend
                .workflow_runs()
                .start(StartWorkflowRunRequest { run_id: run_id.clone() })?;
            let run = harness.wait_terminal(&run_id).await?;
            assert_eq!(run.status(), WorkflowRunStatus::Succeeded, "{}", run.failure_summary());
            Ok(())
        })
    })
}

/// Starting a run without a required Start value is rejected before any node executes.
#[test]
fn starting_without_a_required_start_value_is_rejected() -> TestResult {
    ora_logging::with_trace_logging(|| {
        current_thread_runtime()?.block_on(async {
            let mut harness = WorkflowHarness::open()?;
            let graph = json!({
                "nodes": [
                    {"id":"start","type":"workflow","position":{"x":0,"y":0},"data":{"kind":"start","inputVariables":[
                        {"name":"brief","fieldType":"text-input","valueType":"string","required":true}
                    ]}},
                    {"id":"agent","type":"workflow","position":{"x":360,"y":0},"data":agent_data("ok")},
                    {"id":"out","type":"workflow","position":{"x":720,"y":0},"data":{"kind":"output","outputs":[]}}
                ],
                "edges":[
                    {"source":"start","target":"agent"},
                    {"source":"agent","target":"out"}
                ]
            });
            let run_id = harness.pending_run(graph)?;
            let error = harness
                .backend
                .workflow_runs()
                .start(StartWorkflowRunRequest { run_id })
                .err()
                .ok_or("start accepted a run missing its required Start value")?;
            eprintln!("start rejection without required value: {error}");
            Ok(())
        })
    })
}

// ── Condition nodes ──

/// String equality routes to the matching branch; a non-matching value falls back to the
/// implicit else branch, and the inactive branch never opens a session.
#[test]
fn condition_routes_string_equality_and_falls_back_to_else() -> TestResult {
    ora_logging::with_trace_logging(|| {
        current_thread_runtime()?.block_on(async {
            let mut harness = WorkflowHarness::open()?;
            let graph = json!({
                "nodes": [
                    {"id":"start","type":"workflow","position":{"x":0,"y":0},"data":{"kind":"start","inputVariables":[
                        input("mode","text-input","string")
                    ]}},
                    {"id":"gate","type":"workflow","position":{"x":240,"y":0},"data":{"kind":"condition","cases":[
                        {"id":"fast","logic":"and","conditions":[
                            {"variableSelector":["start","mode"],"operator":"equals","value":"fast"}
                        ]}
                    ]}},
                    {"id":"fast-agent","type":"workflow","position":{"x":480,"y":-120},"data":agent_data("fast-branch")},
                    {"id":"slow-agent","type":"workflow","position":{"x":480,"y":120},"data":agent_data("slow-branch")},
                    {"id":"fast-out","type":"workflow","position":{"x":720,"y":-120},"data":{"kind":"output","outputs":[
                        {"name":"branch","variableSelector":["fast-agent","output"]}
                    ]}},
                    {"id":"slow-out","type":"workflow","position":{"x":720,"y":120},"data":{"kind":"output","outputs":[
                        {"name":"branch","variableSelector":["slow-agent","output"]}
                    ]}}
                ],
                "edges":[
                    {"source":"start","target":"gate"},
                    {"source":"gate","sourceHandle":"fast","target":"fast-agent"},
                    {"source":"fast-agent","target":"fast-out"},
                    {"source":"gate","sourceHandle":"else","target":"slow-agent"},
                    {"source":"slow-agent","target":"slow-out"}
                ]
            });

            for (mode, expected_fragment, absent_fragment) in
                [("fast", "fast-branch", "slow-branch"), ("slow", "slow-branch", "fast-branch")]
            {
                let run = harness.run(graph.clone(), BTreeMap::from([("mode".into(), json!(mode))])).await?;
                assert_eq!(run.status(), WorkflowRunStatus::Succeeded, "{}", run.failure_summary());
                assert_eq!(run.sessions(), 1, "{}", run.failure_summary());
                let outputs = format!(
                    "{}{}",
                    run.node_outputs("fast-agent").join("\n"),
                    run.node_outputs("slow-agent").join("\n"),
                );
                assert!(outputs.contains(expected_fragment), "{outputs:?}");
                assert!(!outputs.contains(absent_fragment), "{outputs:?}");
                let branch = run.run_output()["branch"].as_str().unwrap_or_default().to_string();
                assert!(
                    branch.contains("Fake agent received:") && branch.contains(expected_fragment),
                    "branch output: {branch:?}"
                );
            }
            Ok(())
        })
    })
}

/// Numeric and boolean operators combine under one case's logic, and the first matching case
/// wins when several could match.
#[test]
fn condition_compares_numbers_and_booleans_with_first_match_wins() -> TestResult {
    ora_logging::with_trace_logging(|| {
        current_thread_runtime()?.block_on(async {
            let mut harness = WorkflowHarness::open()?;
            let base_graph = json!({
                "nodes": [
                    {"id":"start","type":"workflow","position":{"x":0,"y":0},"data":{"kind":"start","inputVariables":[
                        input("count","number","number"),
                        input("flag","checkbox","boolean")
                    ]}},
                    {"id":"gate","type":"workflow","position":{"x":240,"y":0},"data":{"kind":"condition","cases":[
                        {"id":"big","logic":"and","conditions":[
                            {"variableSelector":["start","count"],"operator":"greater_than","value":3},
                            {"variableSelector":["start","flag"],"operator":"is","value":true}
                        ]},
                        {"id":"small","logic":"or","conditions":[
                            {"variableSelector":["start","count"],"operator":"less_than","value":3},
                            {"variableSelector":["start","count"],"operator":"equals","value":1}
                        ]}
                    ]}},
                    {"id":"big-agent","type":"workflow","position":{"x":480,"y":-140},"data":agent_data("big-branch")},
                    {"id":"small-agent","type":"workflow","position":{"x":480,"y":0},"data":agent_data("small-branch")},
                    {"id":"mid-agent","type":"workflow","position":{"x":480,"y":140},"data":agent_data("mid-branch")}
                ],
                "edges":[
                    {"source":"start","target":"gate"},
                    {"source":"gate","sourceHandle":"big","target":"big-agent"},
                    {"source":"gate","sourceHandle":"small","target":"small-agent"},
                    {"source":"gate","sourceHandle":"else","target":"mid-agent"}
                ]
            });
            let graph = |count: Value, flag: Value| {
                let mut graph = base_graph.clone();
                graph["nodes"][0]["data"]["inputVariables"] = json!([
                    {"name":"count","fieldType":"number","valueType":"number","value":count},
                    {"name":"flag","fieldType":"checkbox","valueType":"boolean","value":flag}
                ]);
                graph
            };
            for (count, flag, expected) in [
                (json!(5), json!(true), "big-branch"),
                (json!(1), json!(false), "small-branch"),
                (json!(3), json!(false), "mid-branch"),
                (json!(5), json!(false), "mid-branch"),
            ] {
                let run = harness.run(graph(count, flag), BTreeMap::new()).await?;
                assert_eq!(run.status(), WorkflowRunStatus::Succeeded, "{}", run.failure_summary());
                assert_eq!(run.sessions(), 1, "{}", run.failure_summary());
                let outputs = format!(
                    "{}{}{}",
                    run.node_outputs("big-agent").join("\n"),
                    run.node_outputs("small-agent").join("\n"),
                    run.node_outputs("mid-agent").join("\n"),
                );
                assert!(outputs.contains(expected), "expected {expected:?} in {outputs:?}");
            }
            Ok(())
        })
    })
}

// ── Iteration nodes ──

/// Iterating an array of objects binds each round's `item` as an object: nested paths render
/// in prompts and gate Conditions can compare them.
#[test]
fn iteration_over_object_arrays_renders_nested_paths() -> TestResult {
    ora_logging::with_trace_logging(|| {
        current_thread_runtime()?.block_on(async {
            let mut harness = WorkflowHarness::open()?;
            let graph = json!({
                "nodes": [
                    {"id":"start","type":"workflow","position":{"x":0,"y":0},"data":{"kind":"start","inputVariables":[
                        input("rows","json","array[object]")
                    ]}},
                    {"id":"iter","type":"workflow","position":{"x":360,"y":0},"initialWidth":760,"initialHeight":420,"data":{"kind":"iteration","iterationConfig":{
                        "iteratorSelector":["start","rows"],"collectSelector":["body","output"],
                        "errorStrategy":"fail","maxIterations":10
                    }}},
                    {"id":"gate2","type":"workflow","parentId":"iter","position":{"x":420,"y":160},"data":{"kind":"condition","cases":[
                        {"id":"known","logic":"and","conditions":[
                            {"variableSelector":["iter","item","id"],"operator":"greater_than","value":0}
                        ]}
                    ]}},
                    {"id":"body","type":"workflow","parentId":"iter","position":{"x":660,"y":160},"data":agent_data(
                        "note={{#iter.item.note#}} item={{#iter.item#}}"
                    )},
                    {"id":"out","type":"workflow","position":{"x":1240,"y":0},"data":{"kind":"output","outputs":[
                        {"name":"collected","variableSelector":["iter","output"]}
                    ]}}
                ],
                "edges":[
                    {"source":"start","target":"iter"},
                    {"source":"iter","sourceHandle":"iteration-entry","target":"gate2"},
                    {"source":"gate2","sourceHandle":"known","target":"body"},
                    {"source":"iter","target":"out"}
                ]
            });
            let run = harness.run(graph, BTreeMap::from([(
                "rows".into(),
                json!([{"id":1,"note":"alpha"},{"id":2,"note":"beta"}]),
            )])).await?;

            assert_eq!(run.status(), WorkflowRunStatus::Succeeded, "{}", run.failure_summary());
            assert_eq!(run.sessions(), 2, "{}", run.failure_summary());
            let outputs = run.node_outputs("body").join("\n");
            assert!(outputs.contains("note=alpha"), "{outputs:?}");
            assert!(outputs.contains("note=beta"), "{outputs:?}");
            assert!(outputs.contains("item={\"id\":1,\"note\":\"alpha\"}"), "{outputs:?}");
            let collected = run.variable("iter", "output").cloned().unwrap_or_default();
            assert_eq!(collected.as_array().map(Vec::len), Some(2), "{collected:?}");
            assert_eq!(run.run_output()["collected"].as_array().map(Vec::len), Some(2));
            Ok(())
        })
    })
}

/// Under the `continue` strategy a round whose Condition bypasses the collect target settles
/// as failed: the run still succeeds, the failed round is counted, and only executed rounds
/// are collected.
#[test]
fn iteration_round_bypassing_the_collect_target_settles_failed_under_continue() -> TestResult {
    ora_logging::with_trace_logging(|| {
        current_thread_runtime()?.block_on(async {
            let mut harness = WorkflowHarness::open()?;
            let graph = json!({
                "nodes": [
                    {"id":"start","type":"workflow","position":{"x":0,"y":0},"data":{"kind":"start","inputVariables":[
                        input("rows","json","array[object]")
                    ]}},
                    {"id":"iter","type":"workflow","position":{"x":360,"y":0},"initialWidth":760,"initialHeight":420,"data":{"kind":"iteration","iterationConfig":{
                        "iteratorSelector":["start","rows"],"collectSelector":["body","output"],
                        "errorStrategy":"continue","maxIterations":10
                    }}},
                    {"id":"gate2","type":"workflow","parentId":"iter","position":{"x":420,"y":160},"data":{"kind":"condition","cases":[
                        {"id":"big","logic":"and","conditions":[
                            {"variableSelector":["iter","item","id"],"operator":"greater_than","value":1}
                        ]}
                    ]}},
                    {"id":"body","type":"workflow","parentId":"iter","position":{"x":660,"y":160},"data":agent_data("item={{#iter.item.note#}}")},
                    {"id":"out","type":"workflow","position":{"x":1240,"y":0},"data":{"kind":"output","outputs":[
                        {"name":"collected","variableSelector":["iter","output"]},
                        {"name":"failed_count","variableSelector":["iter","failed_count"]}
                    ]}}
                ],
                "edges":[
                    {"source":"start","target":"iter"},
                    {"source":"iter","sourceHandle":"iteration-entry","target":"gate2"},
                    {"source":"gate2","sourceHandle":"big","target":"body"},
                    {"source":"iter","target":"out"}
                ]
            });
            let run = harness.run(graph, BTreeMap::from([(
                "rows".into(),
                json!([{"id":1,"note":"alpha"},{"id":2,"note":"beta"}]),
            )])).await?;

            assert_eq!(run.status(), WorkflowRunStatus::Succeeded, "{}", run.failure_summary());
            assert_eq!(run.sessions(), 1, "only the id=2 round reaches the body; journal: {}", run.journal);
            assert_eq!(run.variable("iter", "failed_count"), Some(&json!(1)));
            let collected = run.variable("iter", "output").cloned().unwrap_or_default();
            assert_eq!(collected.as_array().map(Vec::len), Some(1), "{collected:?}");
            assert_eq!(run.run_output()["failed_count"], json!(1));
            Ok(())
        })
    })
}

/// An empty iterator source is legal: the iteration completes immediately with empty exposed
/// variables and no round ever opens a session.
#[test]
fn iteration_over_an_empty_array_completes_without_rounds() -> TestResult {
    ora_logging::with_trace_logging(|| {
        current_thread_runtime()?.block_on(async {
            let mut harness = WorkflowHarness::open()?;
            let graph = json!({
                "nodes": [
                    {"id":"start","type":"workflow","position":{"x":0,"y":0},"data":{"kind":"start","inputVariables":[
                        input("items","json","array[string]")
                    ]}},
                    {"id":"iter","type":"workflow","position":{"x":360,"y":0},"initialWidth":760,"initialHeight":420,"data":{"kind":"iteration","iterationConfig":{
                        "iteratorSelector":["start","items"],"collectSelector":["body","output"],
                        "errorStrategy":"fail","maxIterations":10
                    }}},
                    {"id":"body","type":"workflow","parentId":"iter","position":{"x":420,"y":160},"data":agent_data("item={{#iter.item#}}")},
                    {"id":"out","type":"workflow","position":{"x":1240,"y":0},"data":{"kind":"output","outputs":[
                        {"name":"collected","variableSelector":["iter","output"]}
                    ]}}
                ],
                "edges":[
                    {"source":"start","target":"iter"},
                    {"source":"iter","sourceHandle":"iteration-entry","target":"body"},
                    {"source":"iter","target":"out"}
                ]
            });
            let run = harness.run(graph, BTreeMap::from([("items".into(), json!([]))])).await?;

            assert_eq!(run.status(), WorkflowRunStatus::Succeeded, "{}", run.failure_summary());
            assert_eq!(run.sessions(), 0, "{}", run.failure_summary());
            assert_eq!(run.variable("iter", "output"), Some(&json!([])));
            assert_eq!(run.variable("iter", "failed_count"), Some(&json!(0)));
            assert_eq!(run.run_output()["collected"], json!([]));
            Ok(())
        })
    })
}

/// A source longer than maxIterations fails at the iteration's startup boundary before any
/// round executes, with an actionable error.
#[test]
fn iteration_fails_fast_when_the_source_exceeds_max_iterations() -> TestResult {
    ora_logging::with_trace_logging(|| {
        current_thread_runtime()?.block_on(async {
            let mut harness = WorkflowHarness::open()?;
            let graph = json!({
                "nodes": [
                    {"id":"start","type":"workflow","position":{"x":0,"y":0},"data":{"kind":"start","inputVariables":[
                        input("items","json","array[string]")
                    ]}},
                    {"id":"iter","type":"workflow","position":{"x":360,"y":0},"initialWidth":760,"initialHeight":420,"data":{"kind":"iteration","iterationConfig":{
                        "iteratorSelector":["start","items"],"collectSelector":["body","output"],
                        "errorStrategy":"fail","maxIterations":2
                    }}},
                    {"id":"body","type":"workflow","parentId":"iter","position":{"x":420,"y":160},"data":agent_data("item={{#iter.item#}}")},
                    {"id":"out","type":"workflow","position":{"x":1240,"y":0},"data":{"kind":"output","outputs":[
                        {"name":"collected","variableSelector":["iter","output"]}
                    ]}}
                ],
                "edges":[
                    {"source":"start","target":"iter"},
                    {"source":"iter","sourceHandle":"iteration-entry","target":"body"},
                    {"source":"iter","target":"out"}
                ]
            });
            let run = harness.run(graph, BTreeMap::from([(
                "items".into(),
                json!(["a", "b", "c"]),
            )])).await?;

            assert_eq!(run.status(), WorkflowRunStatus::Failed, "{}", run.failure_summary());
            assert_eq!(run.sessions(), 0, "{}", run.failure_summary());
            let errors = run.node_errors("iter").join("\n");
            assert!(
                errors.contains("exceeding maxIterations"),
                "iteration error should explain the ceiling; got {errors:?}"
            );
            Ok(())
        })
    })
}

// ── Loop nodes ──

/// The loop exits as soon as its until condition holds, exposing the bound outputs.
#[test]
fn loop_exits_as_soon_as_the_until_condition_holds() -> TestResult {
    ora_logging::with_trace_logging(|| {
        current_thread_runtime()?.block_on(async {
            let mut harness = WorkflowHarness::open()?;
            let graph = json!({
                "schemaVersion": 2,
                "nodes": [
                    {"id":"start","type":"workflow","position":{"x":0,"y":0},"data":{"kind":"start","inputVariables":[]}},
                    {"id":"loop","type":"workflow","position":{"x":360,"y":0},"data":{"kind":"loop","loopConfig":{
                        "maxIterations":3,
                        "variables":[],
                        "until":{"logic":"and","conditions":[
                            {"variableSelector":["writer","output"],"operator":"contains","value":"loop-write"}
                        ]},
                        "outputs":[{"name":"result","variableSelector":["writer","output"]}]
                    }}},
                    {"id":"entry","type":"workflow","parentId":"loop","position":{"x":420,"y":160},"data":{"kind":"start","containerId":"loop"}},
                    {"id":"writer","type":"workflow","parentId":"loop","position":{"x":660,"y":160},"data":loop_agent_data("loop-write","loop")},
                    {"id":"out","type":"workflow","position":{"x":960,"y":0},"data":{"kind":"output","outputs":[
                        {"name":"result","variableSelector":["loop","result"]}
                    ]}}
                ],
                "edges":[
                    {"source":"start","target":"loop"},
                    {"source":"entry","target":"writer"},
                    {"source":"loop","target":"out"}
                ]
            });
            let run = harness.run(graph, BTreeMap::new()).await?;

            assert_eq!(run.status(), WorkflowRunStatus::Succeeded, "{}", run.failure_summary());
            assert_eq!(run.sessions(), 1, "{}", run.failure_summary());
            let result = run
                .variable("loop", "result")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            assert!(result.contains("loop-write"), "loop result: {result:?}");
            let run_result = run.run_output()["result"].as_str().unwrap_or_default().to_string();
            assert!(run_result.contains("loop-write"), "run output: {run_result:?}");
            Ok(())
        })
    })
}

/// A loop that never satisfies its until condition fails at the round ceiling with the round
/// budget named, after exactly maxIterations sessions.
#[test]
fn loop_fails_at_the_round_ceiling_without_termination() -> TestResult {
    ora_logging::with_trace_logging(|| {
        current_thread_runtime()?.block_on(async {
            let mut harness = WorkflowHarness::open()?;
            let graph = json!({
                "schemaVersion": 2,
                "nodes": [
                    {"id":"start","type":"workflow","position":{"x":0,"y":0},"data":{"kind":"start","inputVariables":[]}},
                    {"id":"loop","type":"workflow","position":{"x":360,"y":0},"data":{"kind":"loop","loopConfig":{
                        "maxIterations":2,
                        "variables":[],
                        "until":{"logic":"and","conditions":[
                            {"variableSelector":["writer","output"],"operator":"equals","value":"never-matches"}
                        ]},
                        "outputs":[{"name":"result","variableSelector":["writer","output"]}]
                    }}},
                    {"id":"entry","type":"workflow","parentId":"loop","position":{"x":420,"y":160},"data":{"kind":"start","containerId":"loop"}},
                    {"id":"writer","type":"workflow","parentId":"loop","position":{"x":660,"y":160},"data":loop_agent_data("loop-write","loop")},
                    {"id":"out","type":"workflow","position":{"x":960,"y":0},"data":{"kind":"output","outputs":[
                        {"name":"result","variableSelector":["loop","result"]}
                    ]}}
                ],
                "edges":[
                    {"source":"start","target":"loop"},
                    {"source":"entry","target":"writer"},
                    {"source":"loop","target":"out"}
                ]
            });
            let run = harness.run(graph, BTreeMap::new()).await?;

            assert_eq!(run.status(), WorkflowRunStatus::Failed, "{}", run.failure_summary());
            assert_eq!(run.sessions(), 2, "{}", run.failure_summary());
            let errors = run.node_errors("loop").join("\n");
            assert!(
                errors.contains("did not terminate within 2 rounds"),
                "loop error should name the round budget; got {errors:?}"
            );
            Ok(())
        })
    })
}

/// Typed loop variables carry each round's feedback into the next round's prompt: round two
/// sees round one's output, and the until condition can detect it.
#[test]
fn loop_carries_typed_feedback_into_the_next_round() -> TestResult {
    ora_logging::with_trace_logging(|| {
        current_thread_runtime()?.block_on(async {
            let mut harness = WorkflowHarness::open()?;
            let graph = json!({
                "schemaVersion": 2,
                "nodes": [
                    {"id":"start","type":"workflow","position":{"x":0,"y":0},"data":{"kind":"start","inputVariables":[]}},
                    {"id":"loop","type":"workflow","position":{"x":360,"y":0},"data":{"kind":"loop","loopConfig":{
                        "maxIterations":3,
                        "variables":[{"name":"draft","valueType":"string","initial":{"kind":"constant","value":""},"feedback":["writer","output"]}],
                        "until":{"logic":"and","conditions":[
                            {"variableSelector":["writer","output"],"operator":"contains","value":"draft=Fake agent"}
                        ]},
                        "outputs":[{"name":"result","variableSelector":["writer","output"]}]
                    }}},
                    {"id":"entry","type":"workflow","parentId":"loop","position":{"x":420,"y":160},"data":{"kind":"start","containerId":"loop"}},
                    {"id":"writer","type":"workflow","parentId":"loop","position":{"x":660,"y":160},"data":loop_agent_data("draft={{#loop.draft#}}","loop")},
                    {"id":"out","type":"workflow","position":{"x":960,"y":0},"data":{"kind":"output","outputs":[
                        {"name":"result","variableSelector":["loop","result"]}
                    ]}}
                ],
                "edges":[
                    {"source":"start","target":"loop"},
                    {"source":"entry","target":"writer"},
                    {"source":"loop","target":"out"}
                ]
            });
            let run = harness.run(graph, BTreeMap::new()).await?;

            assert_eq!(run.status(), WorkflowRunStatus::Succeeded, "{}", run.failure_summary());
            assert_eq!(run.sessions(), 2, "{}", run.failure_summary());
            let outputs = run.node_outputs("writer");
            assert_eq!(outputs.len(), 2, "{outputs:?}");
            assert!(
                !outputs[0].contains("draft=Fake agent"),
                "round one starts from the empty initial draft; got {:?}",
                outputs[0]
            );
            assert!(
                outputs[1].contains("draft=Fake agent received:"),
                "round two must embed round one's output; got {:?}",
                outputs[1]
            );
            assert_eq!(run.variable("loop", "result"), Some(&json!(outputs[1])));
            Ok(())
        })
    })
}

// ── Aggregator nodes ──

/// The aggregator passes the first assigned branch output through unchanged, whichever
/// mutually exclusive branch ran, and an assigned `false` is a value, not a missing output.
#[test]
fn aggregator_passes_the_first_assigned_branch_output() -> TestResult {
    ora_logging::with_trace_logging(|| {
        current_thread_runtime()?.block_on(async {
            let mut harness = WorkflowHarness::open()?;
            let branch_graph = |choice: &str| {
                json!({
                    "nodes": [
                        {"id":"start","type":"workflow","position":{"x":0,"y":160},"data":{"kind":"start","inputVariables":[]}},
                        {"id":"condition","type":"workflow","position":{"x":180,"y":160},"data":{"kind":"condition","cases":[
                            {"id":"a","logic":"and","conditions":[
                                {"variableSelector":["route","choice"],"operator":"is","value":"a"}
                            ]}
                        ]}},
                        {"id":"a","type":"workflow","position":{"x":360,"y":80},"data":agent_data("a-branch-work")},
                        {"id":"b","type":"workflow","position":{"x":360,"y":260},"data":agent_data("b-branch-work")},
                        {"id":"aggregator","type":"workflow","position":{"x":560,"y":160},"data":{"kind":"aggregator","aggregatorConfig":{
                            "variables":[["a","output"],["b","output"]]
                        }}},
                        {"id":"out","type":"workflow","position":{"x":760,"y":160},"data":{"kind":"output","outputs":[
                            {"name":"result","variableSelector":["aggregator","output"]}
                        ]}}
                    ],
                    "edges":[
                        {"source":"start","target":"condition"},
                        {"source":"condition","sourceHandle":"a","target":"a"},
                        {"source":"condition","sourceHandle":"else","target":"b"},
                        {"source":"a","target":"aggregator"},
                        {"source":"b","target":"aggregator"},
                        {"source":"aggregator","target":"out"}
                    ],
                    "globalVariables":[{"name":"route.choice","valueType":"string","value":choice}]
                })
            };
            for (choice, expected_fragment) in [("a", "a-branch-work"), ("b", "b-branch-work")] {
                let run = harness.run(branch_graph(choice), BTreeMap::new()).await?;
                assert_eq!(run.status(), WorkflowRunStatus::Succeeded, "{}", run.failure_summary());
                assert_eq!(run.sessions(), 1, "{}", run.failure_summary());
                let aggregated = run
                    .variable("aggregator", "output")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                assert!(aggregated.contains(expected_fragment), "aggregated: {aggregated:?}");
                let run_result = run.run_output()["result"].as_str().unwrap_or_default().to_string();
                assert!(run_result.contains(expected_fragment), "run output: {run_result:?}");
            }

            let false_graph = json!({
                "nodes": [
                    {"id":"start","type":"workflow","position":{"x":0,"y":0},"data":{"kind":"start","inputVariables":[]}},
                    {"id":"aggregator","type":"workflow","position":{"x":360,"y":0},"data":{"kind":"aggregator","aggregatorConfig":{
                        "variables":[["flag","value"]]
                    }}},
                    {"id":"out","type":"workflow","position":{"x":720,"y":0},"data":{"kind":"output","outputs":[
                        {"name":"result","variableSelector":["aggregator","output"]}
                    ]}}
                ],
                "edges":[
                    {"source":"start","target":"aggregator"},
                    {"source":"aggregator","target":"out"}
                ],
                "globalVariables":[{"name":"flag.value","valueType":"boolean","value":false}]
            });
            let run = harness.run(false_graph, BTreeMap::new()).await?;
            assert_eq!(run.status(), WorkflowRunStatus::Succeeded, "{}", run.failure_summary());
            assert_eq!(run.variable("aggregator", "output"), Some(&json!(false)));
            assert_eq!(run.run_output()["result"], json!(false));
            Ok(())
        })
    })
}

// ── Combined pipeline: Condition + Iteration + Loop over typed Start inputs ──

/// The composed pipeline chains every composite control over typed Start inputs: a Condition
/// gate picks the processing branch, an Iteration region filters per-item work under the
/// `continue` strategy, and a Loop refines its draft with typed feedback until its until
/// condition holds. The skipped mode exercises the gate's else path without any session.
#[test]
fn combined_condition_iteration_loop_pipeline() -> TestResult {
    ora_logging::with_trace_logging(|| {
        current_thread_runtime()?.block_on(async {
            let mut harness = WorkflowHarness::open()?;
            let graph = json!({
                "schemaVersion": 2,
                "nodes": [
                    {"id":"start","type":"workflow","position":{"x":0,"y":0},"data":{"kind":"start","inputVariables":[
                        input("mode","text-input","string"),
                        input("threshold","number","number"),
                        input("items","json","array[string]"),
                        input("meta","json","object")
                    ]}},
                    {"id":"gate","type":"workflow","position":{"x":240,"y":0},"data":{"kind":"condition","cases":[
                        {"id":"run","logic":"and","conditions":[
                            {"variableSelector":["start","mode"],"operator":"equals","value":"run"},
                            {"variableSelector":["start","threshold"],"operator":"greater_than_or_equal","value":2}
                        ]}
                    ]}},
                    {"id":"iter","type":"workflow","position":{"x":480,"y":0},"initialWidth":760,"initialHeight":420,"data":{"kind":"iteration","iterationConfig":{
                        "iteratorSelector":["start","items"],"collectSelector":["body","output"],
                        "errorStrategy":"continue","maxIterations":10
                    }}},
                    {"id":"gate2","type":"workflow","parentId":"iter","position":{"x":540,"y":160},"data":{"kind":"condition","cases":[
                        {"id":"go","logic":"and","conditions":[
                            {"variableSelector":["iter","item"],"operator":"not_equals","value":"beta"}
                        ]}
                    ]}},
                    {"id":"body","type":"workflow","parentId":"iter","position":{"x":780,"y":160},"data":agent_data("item={{#iter.item#}} index={{#iter.index#}}")},
                    {"id":"loop","type":"workflow","position":{"x":1320,"y":0},"data":{"kind":"loop","loopConfig":{
                        "maxIterations":3,
                        "variables":[{"name":"draft","valueType":"string","initial":{"kind":"constant","value":""},"feedback":["writer","output"]}],
                        "until":{"logic":"and","conditions":[
                            {"variableSelector":["writer","output"],"operator":"contains","value":"draft=Fake agent"}
                        ]},
                        "outputs":[{"name":"result","variableSelector":["writer","output"]}]
                    }}},
                    {"id":"entry","type":"workflow","parentId":"loop","position":{"x":1380,"y":160},"data":{"kind":"start","containerId":"loop"}},
                    {"id":"writer","type":"workflow","parentId":"loop","position":{"x":1620,"y":160},"data":loop_agent_data(
                        "team={{#start.meta#}} collected={{#iter.output#}} draft={{#loop.draft#}}","loop"
                    )},
                    {"id":"out","type":"workflow","position":{"x":1900,"y":0},"data":{"kind":"output","outputs":[
                        {"name":"collected","variableSelector":["iter","output"]},
                        {"name":"failed_count","variableSelector":["iter","failed_count"]},
                        {"name":"loop_result","variableSelector":["loop","result"]}
                    ]}},
                    {"id":"out2","type":"workflow","position":{"x":1900,"y":260},"data":{"kind":"output","outputs":[
                        {"name":"skipped","variableSelector":["start","mode"]}
                    ]}}
                ],
                "edges":[
                    {"source":"start","target":"gate"},
                    {"source":"gate","sourceHandle":"run","target":"iter"},
                    {"source":"gate","sourceHandle":"else","target":"out2"},
                    {"source":"iter","sourceHandle":"iteration-entry","target":"gate2"},
                    {"source":"gate2","sourceHandle":"go","target":"body"},
                    {"source":"iter","target":"loop"},
                    {"source":"entry","target":"writer"},
                    {"source":"loop","target":"out"}
                ]
            });

            let run = harness.run(graph.clone(), BTreeMap::from([
                ("mode".into(), json!("run")),
                ("threshold".into(), json!(2)),
                ("items".into(), json!(["alpha", "beta", "gamma"])),
                ("meta".into(), json!({"team": "ora"})),
            ])).await?;
            assert_eq!(run.status(), WorkflowRunStatus::Succeeded, "{}", run.failure_summary());
            // Two iteration rounds reach the body (beta is gated away) plus two loop rounds.
            assert_eq!(run.sessions(), 4, "{}", run.failure_summary());
            let body_outputs = run.node_outputs("body").join("\n");
            assert!(body_outputs.contains("item=alpha index=0"), "{body_outputs:?}");
            assert!(body_outputs.contains("item=gamma index=2"), "{body_outputs:?}");
            assert!(!body_outputs.contains("item=beta"), "{body_outputs:?}");
            assert_eq!(run.variable("iter", "failed_count"), Some(&json!(1)));
            let collected = run.variable("iter", "output").cloned().unwrap_or_default();
            assert_eq!(collected.as_array().map(Vec::len), Some(2), "{collected:?}");
            let writer_outputs = run.node_outputs("writer");
            assert_eq!(writer_outputs.len(), 2, "{writer_outputs:?}");
            assert!(
                writer_outputs[1].contains("team={\"team\":\"ora\"}"),
                "the loop must see the object Start value; got {:?}",
                writer_outputs[1]
            );
            assert!(
                writer_outputs[1].contains("collected=[\"Fake agent received:"),
                "the loop must see the iteration's collected output; got {:?}",
                writer_outputs[1]
            );
            assert!(
                writer_outputs[1].contains("item=alpha index=0")
                    && writer_outputs[1].contains("item=gamma index=2"),
                "the collected array must hold both gated-in rounds; got {:?}",
                writer_outputs[1]
            );
            assert!(
                writer_outputs[1].contains("draft=Fake agent received:"),
                "round two must embed round one's output; got {:?}",
                writer_outputs[1]
            );
            assert_eq!(run.variable("loop", "result"), Some(&json!(writer_outputs[1])));
            assert_eq!(run.run_output()["failed_count"], json!(1));
            assert_eq!(run.run_output()["collected"].as_array().map(Vec::len), Some(2));

            let skipped = harness.run(graph, BTreeMap::from([
                ("mode".into(), json!("skip")),
                ("threshold".into(), json!(0)),
                ("items".into(), json!([])),
                ("meta".into(), json!({"team": "ora"})),
            ])).await?;
            assert_eq!(skipped.status(), WorkflowRunStatus::Succeeded, "{}", skipped.failure_summary());
            assert_eq!(skipped.sessions(), 0, "{}", skipped.failure_summary());
            assert_eq!(skipped.run_output()["skipped"], json!("skip"));
            Ok(())
        })
    })
}

/// File-typed Start values reach the prompt as canonical workspace-relative references and
/// unsafe paths are rejected at the run-input boundary before the run starts.
#[test]
fn file_typed_start_values_render_as_references_and_reject_unsafe_paths() -> TestResult {
    ora_logging::with_trace_logging(|| {
        current_thread_runtime()?.block_on(async {
            let mut harness = WorkflowHarness::open()?;
            let graph = json!({
                "nodes": [
                    {"id":"start","type":"workflow","position":{"x":0,"y":0},"data":{"kind":"start","inputVariables":[
                        input("source","file","file"),
                        input("attachments","file-list","array[file]")
                    ]}},
                    {"id":"agent","type":"workflow","position":{"x":360,"y":0},"data":agent_data(
                        "source={{#start.source#}} attachments={{#start.attachments#}}"
                    )},
                    {"id":"out","type":"workflow","position":{"x":720,"y":0},"data":{"kind":"output","outputs":[
                        {"name":"echo","variableSelector":["agent","output"]}
                    ]}}
                ],
                "edges":[
                    {"source":"start","target":"agent"},
                    {"source":"agent","target":"out"}
                ]
            });
            let run_id = harness.pending_run(graph)?;
            for (name, value) in [
                ("source", json!("C:/absolute/secret.txt")),
                ("source", json!("../escape.txt")),
                ("attachments", json!(["ok.txt", "..\\escape.txt"])),
            ] {
                let error = harness
                    .backend
                    .workflow_runs()
                    .update_input(UpdateWorkflowRunInputRequest {
                        run_id: run_id.clone(),
                        input: None,
                        variables: BTreeMap::from([(name.into(), value)]),
                    })
                    .err()
                    .ok_or(format!("update_input accepted the unsafe file path for {name}"))?;
                eprintln!("update_input rejection for unsafe file path: {error}");
            }

            let run = harness
                .run_on_pending(
                    run_id,
                    BTreeMap::from([
                        (
                            "source".into(),
                            json!({"kind":"workspace_file","path":"docs/input.txt"}),
                        ),
                        (
                            "attachments".into(),
                            json!([
                                {"kind":"workspace_file","path":"one.txt"},
                                {"kind":"workspace_file","path":"nested/two.txt"}
                            ]),
                        ),
                    ]),
                )
                .await?;
            assert_eq!(run.status(), WorkflowRunStatus::Succeeded, "{}", run.failure_summary());
            assert_eq!(run.sessions(), 1, "{}", run.failure_summary());
            let output = run.node_outputs("agent").join("\n");
            assert!(
                output.contains("source={\"kind\":\"workspace_file\",\"path\":\"docs/input.txt\"}"),
                "{output:?}"
            );
            assert!(
                output.contains("attachments=[{\"kind\":\"workspace_file\",\"path\":\"one.txt\"},{\"kind\":\"workspace_file\",\"path\":\"nested/two.txt\"}]"),
                "{output:?}"
            );
            Ok(())
        })
    })
}
