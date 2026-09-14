use super::{
    AdvanceWorkflowRunResult, BindWorkflowNodeSessionResult, CancelWorkflowRunResult,
    ExecutionContext, FileChange, NodeExecutor, NodeRunToStart, RestartWorkflowRunResult,
    ResumeWorkflowRunResult, StartWorkflowRunResult, UpdateWorkflowRunInputResult,
    WorkflowGraphNode, WorkflowNodeRunIdGenerator, WorkflowRunEngine, WorkflowRunEngineRepository,
};
use crate::RepositoryError;
use crate::project::Clock;
use ora_domain::{
    AuditFields, SessionId, WorkflowId, WorkflowNodeRun, WorkflowNodeRunId, WorkflowNodeStatus,
    WorkflowRun, WorkflowRunId, WorkflowRunStatus, WorkflowSnapshotId, Workspace, WorkspaceId,
    WorkspaceKind, WorkspaceLifecycle, WorkspaceLocation,
};
use pretty_assertions::assert_eq;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

const GRAPH: &str = r#"{
    "nodes": [
        {"id":"start","data":{"kind":"start"}},
        {"id":"a","data":{"kind":"agent","agentConfig":{"executor":{"agentCli":"c","modelId":"m"},"prompt":"a"}}},
        {"id":"b","data":{"kind":"agent","agentConfig":{"executor":{"agentCli":"c","modelId":"m"},"prompt":"b"}}},
        {"id":"c","data":{"kind":"agent","agentConfig":{"executor":{"agentCli":"c","modelId":"m"},"prompt":"c"}}},
        {"id":"out","data":{"kind":"output"}}
    ],
    "edges": [
        {"source":"start","target":"a"},
        {"source":"start","target":"b"},
        {"source":"a","target":"c"},
        {"source":"c","target":"out"},
        {"source":"b","target":"out"}
    ]
}"#;

struct NoopExecutor;

impl NodeExecutor for NoopExecutor {
    fn dispatch(
        &self,
        _node_run_id: &WorkflowNodeRunId,
        _node: &WorkflowGraphNode,
        _context: &ExecutionContext,
    ) {
    }
}

struct SeqGen;

impl WorkflowNodeRunIdGenerator for SeqGen {
    fn generate_node_run_id(&self) -> WorkflowNodeRunId {
        WorkflowNodeRunId::new("unused")
    }
}

struct ClockAt(i64);

impl Clock for ClockAt {
    fn now_timestamp_millis(&self) -> i64 {
        self.0
    }
}

/// Records the node ids the engine asks the repository to clear.
struct RecordingRepository {
    context: ExecutionContext,
    node_runs: Vec<WorkflowNodeRun>,
    cleared: Arc<Mutex<Option<Vec<String>>>>,
}

impl WorkflowRunEngineRepository for RecordingRepository {
    fn find_execution_context(
        &self,
        _run_id: &WorkflowRunId,
    ) -> Result<Option<ExecutionContext>, RepositoryError> {
        Ok(Some(self.context.clone()))
    }

    fn list_node_runs(
        &self,
        _run_id: &WorkflowRunId,
    ) -> Result<Vec<WorkflowNodeRun>, RepositoryError> {
        Ok(self.node_runs.clone())
    }

    fn find_last_failed_attempt(
        &self,
        _run_id: &WorkflowRunId,
        _node_id: &str,
    ) -> Result<Option<WorkflowNodeRun>, RepositoryError> {
        Ok(None)
    }

    fn bind_node_run_session(
        &self,
        _node_run_id: &WorkflowNodeRunId,
        _session_id: &SessionId,
        _now: i64,
    ) -> Result<BindWorkflowNodeSessionResult, RepositoryError> {
        Ok(BindWorkflowNodeSessionResult::NotFound)
    }

    fn find_node_run_by_session_id(
        &self,
        _session_id: &SessionId,
    ) -> Result<Option<WorkflowNodeRun>, RepositoryError> {
        Ok(None)
    }

    fn find_node_run_by_id(
        &self,
        _node_run_id: &WorkflowNodeRunId,
    ) -> Result<Option<WorkflowNodeRun>, RepositoryError> {
        Ok(None)
    }

    fn transition_node_run_status(
        &self,
        _node_run_id: &WorkflowNodeRunId,
        _from: WorkflowNodeStatus,
        _to: WorkflowNodeStatus,
        _now: i64,
    ) -> Result<AdvanceWorkflowRunResult, RepositoryError> {
        Ok(AdvanceWorkflowRunResult::NotFound)
    }

    fn start_run(
        &self,
        _run_id: &WorkflowRunId,
        _start_node_run: &NodeRunToStart,
        _now: i64,
    ) -> Result<StartWorkflowRunResult, RepositoryError> {
        Ok(StartWorkflowRunResult::Current)
    }

    fn start_ready_nodes(
        &self,
        _run_id: &WorkflowRunId,
        _node_runs: &[NodeRunToStart],
        _now: i64,
    ) -> Result<(), RepositoryError> {
        Ok(())
    }

    fn complete_node(
        &self,
        _node_run_id: &WorkflowNodeRunId,
        _output: Option<String>,
        _structured_output: Option<serde_json::Value>,
        _stop_reason: Option<String>,
        _file_changes: Vec<FileChange>,
        _now: i64,
    ) -> Result<AdvanceWorkflowRunResult, RepositoryError> {
        Ok(AdvanceWorkflowRunResult::NotFound)
    }

    fn fail_node(
        &self,
        _node_run_id: &WorkflowNodeRunId,
        _failure: super::NodeFailure,
        _now: i64,
    ) -> Result<AdvanceWorkflowRunResult, RepositoryError> {
        Ok(AdvanceWorkflowRunResult::NotFound)
    }

    fn record_node_checkpoint(
        &self,
        _node_run_id: &WorkflowNodeRunId,
        _checkpoint: Option<&str>,
        _checkpoint_error: Option<&str>,
        _now: i64,
    ) -> Result<(), RepositoryError> {
        Ok(())
    }

    fn finish_run(
        &self,
        _run_id: &WorkflowRunId,
        _output: Option<String>,
        _now: i64,
    ) -> Result<(), RepositoryError> {
        Ok(())
    }

    fn cancel_run(
        &self,
        _run_id: &WorkflowRunId,
        _now: i64,
    ) -> Result<CancelWorkflowRunResult, RepositoryError> {
        Ok(CancelWorkflowRunResult::NotFound)
    }

    fn restart_run(
        &self,
        _run_id: &WorkflowRunId,
        _now: i64,
    ) -> Result<RestartWorkflowRunResult, RepositoryError> {
        Ok(RestartWorkflowRunResult::NotFound)
    }

    fn resume_from_failure(
        &self,
        _run_id: &WorkflowRunId,
        node_ids_to_clear: &[String],
        _now: i64,
    ) -> Result<ResumeWorkflowRunResult, RepositoryError> {
        *self.cleared.lock().expect("cleared lock") = Some(node_ids_to_clear.to_vec());
        // The persisted context stays Failed, so the follow-up schedule wave is a no-op.
        Ok(ResumeWorkflowRunResult::Resumed)
    }

    fn update_run_input(
        &self,
        _run_id: &WorkflowRunId,
        _input: Option<String>,
        _variables: BTreeMap<String, serde_json::Value>,
        _now: i64,
    ) -> Result<UpdateWorkflowRunInputResult, RepositoryError> {
        Ok(UpdateWorkflowRunInputResult::NotFound)
    }

    fn list_recoverable_runs(&self) -> Result<Vec<WorkflowRunId>, RepositoryError> {
        Ok(Vec::new())
    }

    fn fail_orphaned_node_runs(
        &self,
        _run_ids: &[WorkflowRunId],
        _now: i64,
    ) -> Result<(), RepositoryError> {
        Ok(())
    }
}

fn execution_context() -> ExecutionContext {
    ExecutionContext {
        run: WorkflowRun::new(
            WorkflowRunId::new("run-1"),
            WorkspaceId::new("workspace-1"),
            WorkflowId::new("workflow-1"),
            WorkflowSnapshotId::new("snapshot-1"),
            "Review",
            WorkflowRunStatus::Failed,
            Some(r#"{"current_nodes":["a"]}"#.to_string()),
            Some("task".to_string()),
            None,
            Some("a failed".to_string()),
            None,
            Some(10),
            Some(20),
            AuditFields::new(1, 1, false),
        ),
        workspace: Workspace::new(
            WorkspaceId::new("workspace-1"),
            ora_domain::ProjectId::new("project-1"),
            WorkspaceKind::Main,
            WorkspaceLocation::local_filesystem("/tmp/workspace"),
            WorkspaceLifecycle::Active,
            AuditFields::new(1, 1, false),
        ),
        graph_json: GRAPH.to_string(),
    }
}

fn node_run(id: &str, node_id: &str, status: WorkflowNodeStatus) -> WorkflowNodeRun {
    WorkflowNodeRun::new(
        WorkflowNodeRunId::new(id),
        WorkflowRunId::new("run-1"),
        node_id,
        "agent",
        None,
        status,
        None,
        None,
        None,
        None,
        Some(1),
        None,
        AuditFields::new(1, 1, false),
    )
}

fn engine(
    node_runs: Vec<WorkflowNodeRun>,
) -> (
    WorkflowRunEngine<RecordingRepository, NoopExecutor, SeqGen, ClockAt>,
    Arc<Mutex<Option<Vec<String>>>>,
) {
    let cleared = Arc::new(Mutex::new(None));
    let repository = RecordingRepository {
        context: execution_context(),
        node_runs,
        cleared: cleared.clone(),
    };
    (
        WorkflowRunEngine::new(repository, NoopExecutor, SeqGen, ClockAt(40)),
        cleared,
    )
}

/// A run with only succeeded node runs has nothing to resume from.
#[test]
fn resume_from_failure_is_not_resumable_without_failed_nodes() {
    let (engine, cleared) = engine(vec![
        node_run("nr-start", "start", WorkflowNodeStatus::Succeeded),
        node_run("nr-a", "a", WorkflowNodeStatus::Succeeded),
        node_run("nr-b", "b", WorkflowNodeStatus::Succeeded),
    ]);
    assert_eq!(
        engine
            .resume_from_failure(&WorkflowRunId::new("run-1"))
            .unwrap(),
        ResumeWorkflowRunResult::NotResumable
    );
    assert_eq!(*cleared.lock().expect("cleared lock"), None);
}

/// The engine clears the failed node and every transitive successor, not the sibling branch.
#[test]
fn resume_from_failure_clears_failed_nodes_and_successors_not_siblings() {
    let (engine, cleared) = engine(vec![
        node_run("nr-start", "start", WorkflowNodeStatus::Succeeded),
        node_run("nr-a", "a", WorkflowNodeStatus::Failed),
        node_run("nr-b", "b", WorkflowNodeStatus::Succeeded),
    ]);
    assert_eq!(
        engine
            .resume_from_failure(&WorkflowRunId::new("run-1"))
            .unwrap(),
        ResumeWorkflowRunResult::Resumed
    );
    assert_eq!(
        *cleared.lock().expect("cleared lock"),
        Some(vec!["a".to_string(), "c".to_string(), "out".to_string()])
    );
}
