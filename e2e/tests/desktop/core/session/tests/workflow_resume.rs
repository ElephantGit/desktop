//! One full resume-from-failure flow through the public Tauri workflow commands.

use super::{
    agent_ref, current_thread_runtime, install_fake_opencode_plugin, main_workspace_id,
    open_ready_backend,
};
use crate::setup::DesktopTestSetup;
use ora_contracts::*;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::fs;
use std::time::{Duration, Instant};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Keeps setup, the fake ACP agent, and the public commands under one TRACE-scoped runtime.
fn run_case(test: impl std::future::Future<Output = TestResult>) -> TestResult {
    ora_logging::with_trace_logging(|| {
        current_thread_runtime()?
            .block_on(async { tokio::time::timeout(Duration::from_secs(15), test).await? })
    })
}

/// Polls run status without blocking the current-thread runtime that drives agent dispatch.
async fn wait_run_status(
    runs: &ora_backend::WorkflowRuns,
    run_id: &str,
    expected: WorkflowRunStatus,
) -> TestResult {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let detail = runs.get(GetWorkflowRunRequest {
            run_id: run_id.to_string(),
        })?;
        if detail.run.status == expected {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "run {} stayed {:?} instead of {expected:?}",
                run_id, detail.run.status
            )
            .into());
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

/// Creates a two-agent linear graph whose second node requires `{"ok": true}` JSON.
fn structured_second_node_graph() -> String {
    json!({
        "nodes": [
            {"id": "start", "data": {"kind": "start"}},
            {"id": "first", "data": {"kind": "agent", "agentConfig": {
                "executor": {"agentCli": agent_ref(), "modelId": "anthropic/claude-sonnet-4"},
                "prompt": "first"
            }}},
            {"id": "second", "data": {"kind": "agent", "agentConfig": {
                "executor": {"agentCli": agent_ref(), "modelId": "anthropic/claude-sonnet-4"},
                "prompt": "second",
                "outputContract": {
                    "type": "structured",
                    "textExposure": "includeFinalText",
                    "schema": {
                        "type": "object",
                        "properties": {"ok": {"type": "boolean"}},
                        "required": ["ok"]
                    }
                }
            }}},
            {"id": "output", "data": {"kind": "output"}}
        ],
        "edges": [
            {"source": "start", "target": "first"},
            {"source": "first", "target": "second"},
            {"source": "second", "target": "output"}
        ]
    })
    .to_string()
}

/// E1: fail the structured second node, resume with keep, and succeed on the injected retry.
#[test]
fn resume_from_failure_retries_the_structured_node_after_injected_context() -> TestResult {
    run_case(async {
        let setup = DesktopTestSetup::new()?;
        install_fake_opencode_plugin(&setup.backend_paths().home_directory)?;
        let backend = open_ready_backend(&setup)?;
        let workspace = setup.root().join("workspace");
        fs::create_dir_all(&workspace)?;
        backend.projects().create(CreateProjectRequest {
            name: "Resume E2E".to_string(),
            main_workspace_path: workspace.to_string_lossy().into_owned(),
        })?;
        let workspace_id = main_workspace_id(&backend)?;
        let workflow = backend
            .workflows()
            .create(CreateWorkflowRequest {
                name: "Resume linear".to_string(),
                graph: Some(structured_second_node_graph()),
            })?
            .workflow;
        backend.workflows().publish(PublishWorkflowRequest {
            workflow_id: workflow.id.clone(),
            version: Some("v1".to_string()),
        })?;
        let runs = backend.workflow_runs();
        let run = runs
            .create(CreateWorkflowRunRequest {
                workspace_id,
                workflow_id: workflow.id,
                locale: WorkflowRunLocale::EnUs,
                snapshot_id: None,
                kickoff_input: None,
                name: None,
                inject_last_failure: None,
            })?
            .run;
        runs.start(StartWorkflowRunRequest {
            run_id: run.id.clone(),
        })?;
        wait_run_status(&runs, &run.id, WorkflowRunStatus::Failed).await?;
        let failed = runs.get(GetWorkflowRunRequest {
            run_id: run.id.clone(),
        })?;
        let first_failed = failed
            .nodes
            .iter()
            .find(|node| node.node_id == "first")
            .ok_or("missing first node")?;
        let first_id = first_failed.id.clone();
        let first_started = first_failed.started_at;
        assert_eq!(first_failed.status, WorkflowNodeStatus::Succeeded);
        let preview = runs.preview_resume(PreviewWorkflowRunResumeRequest {
            run_id: run.id.clone(),
        })?;
        assert!(preview.resumable);
        runs.resume_from_failure(ResumeWorkflowRunRequest {
            run_id: run.id.clone(),
            rollback: Some(ResumeRollbackMode::Keep),
            snapshot_id: None,
        })?;
        wait_run_status(&runs, &run.id, WorkflowRunStatus::Succeeded).await?;
        let succeeded = runs.get(GetWorkflowRunRequest {
            run_id: run.id.clone(),
        })?;
        let first_live = succeeded
            .nodes
            .iter()
            .find(|node| node.node_id == "first")
            .ok_or("missing first node after resume")?;
        assert_eq!(first_live.id, first_id);
        assert_eq!(first_live.started_at, first_started);
        let second_live = succeeded
            .nodes
            .iter()
            .find(|node| node.node_id == "second")
            .ok_or("missing second node after resume")?;
        assert_eq!(second_live.status, WorkflowNodeStatus::Succeeded);
        let payload: serde_json::Value = serde_json::from_str(
            second_live
                .payload
                .as_deref()
                .ok_or("missing second node payload")?,
        )?;
        assert!(
            payload
                .get("injected_failure_context")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|text| text.contains("Previous attempt")),
            "{payload}"
        );
        Ok(())
    })
}
