//! Plans and applies worktree rollback before a failed workflow run is resumed.

use super::checkpoint::changes_since_checkpoint;
use super::engine::ConcreteWorkflowRunControl;
use crate::agent_runtime::AgentRuntimeManager;
use crate::error::{BackendError, ErrorClassification};
use gitlancer::git::worktree::FindWorktreeRequest;
use gitlancer::{
    CliGitRunner, Git, GitlancerError, RepoRoot, RestoreAllRequest, RestorePathsRequest,
    SnapshotWorktreeRequest,
};
use ora_application::{ApplicationError, FileChange, WorkflowRunEngineRepository};
use ora_contracts::{
    EmptyErrorParams, PreviewWorkflowRunResumeRequest, PreviewWorkflowRunResumeResponse,
    PublicError, ResumeFailedNodePreview, ResumeRollbackMode, ResumeWorkflowRunRequest,
    ResumeWorkflowRunResponse, WorkflowFileChange,
};
use ora_db::{RepositoryPool, SqliteWorkflowRunEngineRepository};
use ora_domain::{WorkflowNodeStatus, WorkflowRunId, WorkflowRunStatus, WorkspaceId};
use ora_logging::ora_info;
use std::cmp::Ordering;
use std::path::Path;

/// One failed or cancelled node together with the checkpoint recorded on its payload.
pub(super) struct FailedNodeCheckpoint {
    pub node_id: String,
    pub node_run_id: String,
    pub started_at: Option<i64>,
    pub checkpoint: Option<String>,
    pub checkpoint_error: Option<String>,
    pub node_file_changes: Vec<FileChange>,
}

/// Derived rollback availability for one run, with failed nodes ordered by `started_at` ascending
/// and missing timestamps last so a later restore can let the earliest checkpoint win.
pub(super) struct RollbackPlan {
    pub run_status: WorkflowRunStatus,
    pub has_running_node: bool,
    pub failed: Vec<FailedNodeCheckpoint>,
    pub checkpoint_available: bool,
    pub checkpoint_unavailable_reason: Option<&'static str>,
}

impl RollbackPlan {
    /// A run can resume when it is failed or cancelled, idle, and has at least one failed node.
    pub(super) fn resumable(&self) -> bool {
        matches!(
            self.run_status,
            WorkflowRunStatus::Failed | WorkflowRunStatus::Cancelled
        ) && !self.has_running_node
            && !self.failed.is_empty()
    }

    /// Node-file rollback needs a checkpoint on every failed node.
    pub(super) fn node_files_available(&self) -> bool {
        self.resumable() && self.failed.iter().all(|node| node.checkpoint.is_some())
    }
}

/// Reads the live run and node rows and classifies which rollback modes are available.
pub(super) fn plan_rollback(
    pool: &RepositoryPool,
    run_id: &WorkflowRunId,
) -> Result<RollbackPlan, BackendError> {
    let repository = SqliteWorkflowRunEngineRepository::new(pool.clone());
    let context = repository
        .find_execution_context(run_id)
        .map_err(|error| BackendError::internal("failed to load workflow run for rollback", error))?
        .ok_or_else(|| {
            BackendError::from(ApplicationError::WorkflowRunNotFound {
                run_id: run_id.to_string(),
            })
        })?;
    let node_runs = repository
        .list_node_runs(run_id)
        .map_err(|error| BackendError::internal("failed to list node runs for rollback", error))?;
    let has_running_node = node_runs
        .iter()
        .any(|node_run| node_run.status == WorkflowNodeStatus::Running);
    let mut failed: Vec<FailedNodeCheckpoint> = node_runs
        .iter()
        .filter(|node_run| {
            matches!(
                node_run.status,
                WorkflowNodeStatus::Failed | WorkflowNodeStatus::Cancelled
            )
        })
        .map(|node_run| {
            let payload = parse_node_payload(node_run.payload.as_deref());
            FailedNodeCheckpoint {
                node_id: node_run.node_id.clone(),
                node_run_id: node_run.id.to_string(),
                started_at: node_run.started_at,
                checkpoint: payload.checkpoint,
                checkpoint_error: payload.checkpoint_error,
                node_file_changes: payload.file_changes,
            }
        })
        .collect();
    failed.sort_by(|left, right| match (left.started_at, right.started_at) {
        (Some(left_at), Some(right_at)) => left_at.cmp(&right_at),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    });
    let failed_ids: std::collections::HashSet<&str> =
        failed.iter().map(|node| node.node_id.as_str()).collect();
    let resumable = matches!(
        context.run.status,
        WorkflowRunStatus::Failed | WorkflowRunStatus::Cancelled
    ) && !has_running_node
        && !failed.is_empty();
    let node_files_available = resumable && failed.iter().all(|node| node.checkpoint.is_some());
    let earliest_failed_started_at = failed.iter().find_map(|node| node.started_at);
    let siblings_ran_after = earliest_failed_started_at.is_some_and(|earliest| {
        node_runs.iter().any(|node_run| {
            !failed_ids.contains(node_run.node_id.as_str())
                && node_run
                    .started_at
                    .is_some_and(|started_at| started_at > earliest)
        })
    });
    let (checkpoint_available, checkpoint_unavailable_reason) = if !resumable {
        (false, Some("not_resumable"))
    } else if !node_files_available {
        (false, Some("no_checkpoint"))
    } else if siblings_ran_after {
        (false, Some("siblings_ran_after_checkpoint"))
    } else {
        (true, None)
    };
    Ok(RollbackPlan {
        run_status: context.run.status,
        has_running_node,
        failed,
        checkpoint_available,
        checkpoint_unavailable_reason,
    })
}

/// Builds the public resume preview, including a live diff against each failed node's checkpoint.
pub(super) fn preview(
    pool: &RepositoryPool,
    workspace_root: &Path,
    run_id: &WorkflowRunId,
) -> Result<PreviewWorkflowRunResumeResponse, BackendError> {
    let plan = plan_rollback(pool, run_id)?;
    let failed_nodes = plan
        .failed
        .iter()
        .map(|node| ResumeFailedNodePreview {
            node_id: node.node_id.clone(),
            node_run_id: node.node_run_id.clone(),
            started_at: node.started_at,
            checkpoint: node.checkpoint.clone(),
            checkpoint_error: node.checkpoint_error.clone(),
            node_file_changes: node
                .node_file_changes
                .iter()
                .cloned()
                .map(to_contract_change)
                .collect(),
            changed_since_checkpoint: node
                .checkpoint
                .as_deref()
                .map(|oid| changes_since_checkpoint(workspace_root, oid))
                .unwrap_or_default()
                .into_iter()
                .map(to_contract_change)
                .collect(),
        })
        .collect();
    Ok(PreviewWorkflowRunResumeResponse {
        resumable: plan.resumable(),
        failed_nodes,
        node_files_available: plan.node_files_available(),
        checkpoint_available: plan.checkpoint_available,
        checkpoint_unavailable_reason: plan.checkpoint_unavailable_reason.map(str::to_string),
        current_snapshot_id: String::new(),
        current_snapshot_version: String::new(),
        published_snapshot_id: None,
        published_snapshot_version: None,
        published_snapshot_switchable: false,
        published_snapshot_incompatible_reason: None,
    })
}

/// Applies the requested rollback. `Keep` touches nothing; other modes snapshot first so the
/// operator can undo, but only after the mode is known to be available so a rejection cannot
/// leave a pre-rollback ref behind.
pub(super) fn apply_rollback(
    workspace_root: &Path,
    plan: &RollbackPlan,
    mode: ResumeRollbackMode,
    run_id: &WorkflowRunId,
    now: i64,
) -> Result<Option<String>, BackendError> {
    match mode {
        ResumeRollbackMode::Keep => Ok(None),
        ResumeRollbackMode::NodeFiles => {
            if !plan.node_files_available() {
                return Err(not_resumable());
            }
            let pre_rollback = snapshot_pre_rollback(workspace_root, run_id, now)?;
            let git = open_git(workspace_root)?;
            let mut restored = 0usize;
            let mut deleted = 0usize;
            for node in plan.failed.iter().rev() {
                let Some(checkpoint) = node.checkpoint.as_deref() else {
                    continue;
                };
                let paths: Vec<String> = node
                    .node_file_changes
                    .iter()
                    .map(|change| change.path.clone())
                    .collect();
                if paths.is_empty() {
                    continue;
                }
                let response = git
                    .git
                    .restore_paths(RestorePathsRequest {
                        worktree: &git.worktree,
                        commit_oid: checkpoint,
                        paths: &paths,
                    })
                    .map_err(|error| {
                        git_failure(format!(
                            "failed to restore node files for run {run_id} node {}: {error}",
                            node.node_id
                        ))
                    })?;
                restored += response.restored.len();
                deleted += response.deleted.len();
            }
            ora_info!(
                run_id = %run_id,
                mode = "node_files",
                pre_rollback_oid = %pre_rollback,
                restored,
                deleted,
                "applied workflow resume rollback"
            );
            Ok(Some(pre_rollback))
        }
        ResumeRollbackMode::Checkpoint => {
            if !plan.checkpoint_available {
                return Err(not_resumable());
            }
            let Some(earliest) = plan.failed.first() else {
                return Err(not_resumable());
            };
            let Some(checkpoint) = earliest.checkpoint.as_deref() else {
                return Err(not_resumable());
            };
            let pre_rollback = snapshot_pre_rollback(workspace_root, run_id, now)?;
            let git = open_git(workspace_root)?;
            let response = git
                .git
                .restore_all(RestoreAllRequest {
                    worktree: &git.worktree,
                    commit_oid: checkpoint,
                })
                .map_err(|error| {
                    git_failure(format!(
                        "failed to restore checkpoint for run {run_id}: {error}"
                    ))
                })?;
            ora_info!(
                run_id = %run_id,
                mode = "checkpoint",
                pre_rollback_oid = %pre_rollback,
                restored = response.restored.len(),
                deleted = response.deleted.len(),
                "applied workflow resume rollback"
            );
            Ok(Some(pre_rollback))
        }
    }
}

/// Plans and applies rollback, then optionally switches snapshot, then resumes through the engine.
pub(super) fn resume_from_failure(
    pool: &RepositoryPool,
    agent_runtime: &AgentRuntimeManager,
    skills_root: &Path,
    engine: &ConcreteWorkflowRunControl,
    request: ResumeWorkflowRunRequest,
    now: i64,
) -> Result<ResumeWorkflowRunResponse, BackendError> {
    let mode = request.rollback.unwrap_or(ResumeRollbackMode::Keep);
    let run_id = WorkflowRunId::new(&request.run_id);
    let needs_workspace = mode != ResumeRollbackMode::Keep || request.snapshot_id.is_some();
    let workspace_root = if needs_workspace {
        let workspace_id = load_workspace_id(pool, &run_id)?;
        Some(agent_runtime.workspace_cwd(&workspace_id)?)
    } else {
        None
    };
    let pre_rollback_checkpoint = match (mode, workspace_root.as_deref()) {
        (ResumeRollbackMode::Keep, _) => None,
        (_, Some(workspace_root)) => {
            let plan = plan_rollback(pool, &run_id)?;
            if !plan.resumable() {
                return Err(not_resumable());
            }
            apply_rollback(workspace_root, &plan, mode, &run_id, now)?
        }
        (_, None) => return Err(not_resumable()),
    };
    if let Some(workspace_root) = workspace_root.as_deref() {
        super::snapshot_switch::switch_if_requested(
            pool,
            skills_root,
            workspace_root,
            &run_id,
            request.snapshot_id.as_deref(),
            now,
        )?;
    }
    let ResumeWorkflowRunResponse { run, .. } = engine.resume_from_failure(request)?;
    Ok(ResumeWorkflowRunResponse {
        run,
        pre_rollback_checkpoint,
    })
}

/// Resolves the run's worktree and returns the live rollback preview.
pub(super) fn preview_resume(
    pool: &RepositoryPool,
    agent_runtime: &AgentRuntimeManager,
    request: PreviewWorkflowRunResumeRequest,
) -> Result<PreviewWorkflowRunResumeResponse, BackendError> {
    let run_id = WorkflowRunId::new(&request.run_id);
    let workspace_id = load_workspace_id(pool, &run_id)?;
    let workspace_root = agent_runtime.workspace_cwd(&workspace_id)?;
    let mut response = preview(pool, &workspace_root, &run_id)?;
    fill_snapshot_preview(pool, &run_id, &mut response)?;
    Ok(response)
}

pub(super) fn fill_snapshot_preview(
    pool: &RepositoryPool,
    run_id: &WorkflowRunId,
    response: &mut PreviewWorkflowRunResumeResponse,
) -> Result<(), BackendError> {
    let context = super::snapshot_switch::load_context(pool, run_id)?;
    response.current_snapshot_id = context.current.id.to_string();
    response.current_snapshot_version = context.current.version.clone();
    let Some(published) = context.published.as_ref() else {
        return Ok(());
    };
    response.published_snapshot_id = Some(published.id.to_string());
    response.published_snapshot_version = Some(published.version.clone());
    if published.id == context.current.id {
        response.published_snapshot_switchable = false;
        response.published_snapshot_incompatible_reason = None;
        return Ok(());
    }
    match super::snapshot_switch::check_switch(pool, &context, published) {
        Ok(_) => {
            response.published_snapshot_switchable = true;
            response.published_snapshot_incompatible_reason = None;
        }
        Err(error) => {
            if let PublicError::WorkflowSnapshotIncompatibleWithResume(params) =
                error.public_error()
            {
                response.published_snapshot_switchable = false;
                response.published_snapshot_incompatible_reason = Some(params.reason.clone());
            } else {
                return Err(error);
            }
        }
    }
    Ok(())
}

struct ParsedPayload {
    checkpoint: Option<String>,
    checkpoint_error: Option<String>,
    file_changes: Vec<FileChange>,
}

#[derive(serde::Deserialize)]
struct PayloadView {
    checkpoint: Option<String>,
    checkpoint_error: Option<String>,
    #[serde(default)]
    file_changes: Vec<FileChangeView>,
}

#[derive(serde::Deserialize)]
struct FileChangeView {
    path: String,
    #[serde(default)]
    additions: u64,
    #[serde(default)]
    deletions: u64,
}

/// Reads checkpoint provenance and recorded file changes out of a node-run payload blob.
fn parse_node_payload(payload: Option<&str>) -> ParsedPayload {
    let Some(payload) = payload else {
        return ParsedPayload {
            checkpoint: None,
            checkpoint_error: None,
            file_changes: Vec::new(),
        };
    };
    let Ok(view) = serde_json::from_str::<PayloadView>(payload) else {
        return ParsedPayload {
            checkpoint: None,
            checkpoint_error: None,
            file_changes: Vec::new(),
        };
    };
    ParsedPayload {
        checkpoint: view.checkpoint,
        checkpoint_error: view.checkpoint_error,
        file_changes: view
            .file_changes
            .into_iter()
            .map(|change| FileChange {
                path: change.path,
                additions: change.additions,
                deletions: change.deletions,
            })
            .collect(),
    }
}

fn to_contract_change(change: FileChange) -> WorkflowFileChange {
    WorkflowFileChange {
        path: change.path,
        additions: change.additions,
        deletions: change.deletions,
    }
}

fn not_resumable() -> BackendError {
    BackendError::from(ApplicationError::WorkflowRunNotResumable)
}

fn git_failure(message: String) -> BackendError {
    BackendError::new(
        ErrorClassification::Internal,
        PublicError::InternalError(EmptyErrorParams {}),
        message,
    )
}

fn load_workspace_id(
    pool: &RepositoryPool,
    run_id: &WorkflowRunId,
) -> Result<WorkspaceId, BackendError> {
    let repository = SqliteWorkflowRunEngineRepository::new(pool.clone());
    let context = repository
        .find_execution_context(run_id)
        .map_err(|error| BackendError::internal("failed to load workflow run workspace", error))?
        .ok_or_else(|| {
            BackendError::from(ApplicationError::WorkflowRunNotFound {
                run_id: run_id.to_string(),
            })
        })?;
    Ok(context.run.workspace_id)
}

struct OpenGit {
    git: Git<CliGitRunner>,
    worktree: gitlancer::WorktreeHandle,
}

fn open_git(workspace_root: &Path) -> Result<OpenGit, BackendError> {
    discover_git(workspace_root).map_err(|error| {
        git_failure(format!(
            "failed to open git worktree at {}: {error}",
            workspace_root.display()
        ))
    })
}

fn discover_git(workspace_root: &Path) -> Result<OpenGit, GitlancerError> {
    let git = Git::new(CliGitRunner);
    let repository = git.discover_repository(RepoRoot::new(workspace_root))?;
    let worktree = git.find_worktree(FindWorktreeRequest {
        repository: &repository,
        candidate_path: workspace_root,
    })?;
    Ok(OpenGit { git, worktree })
}

fn snapshot_pre_rollback(
    workspace_root: &Path,
    run_id: &WorkflowRunId,
    now: i64,
) -> Result<String, BackendError> {
    let git = open_git(workspace_root)?;
    let name = format!("pre-rollback-{run_id}-{now}");
    let message = format!("ora pre-rollback checkpoint: run {run_id}");
    let response = git
        .git
        .snapshot_worktree(SnapshotWorktreeRequest {
            worktree: &git.worktree,
            name: &name,
            message: &message,
        })
        .map_err(|error| {
            git_failure(format!(
                "failed to write pre-rollback checkpoint for run {run_id}: {error}"
            ))
        })?;
    Ok(response.commit_oid)
}
