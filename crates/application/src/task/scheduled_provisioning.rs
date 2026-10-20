use super::handlers::provision_worktree_task;
use super::mapper::map_task;
use super::ports::{TaskIdGenerator, TaskWorktreeProvisioner};
use super::provisioning::{
    ScheduledRunWorktreeLink, TaskWorkspaceCommit, WorktreeProvisioningLeaseStore,
};
use crate::{ApplicationError, Clock};
use ora_contracts::Task;
use ora_domain::{ProjectId, ScheduledRunId};
use std::path::PathBuf;

/// Provisions the isolated worktree task whose ownership commit also binds the
/// Scheduled Run, so the run row and the task rows become visible together.
pub struct CreateScheduledWorktreeTaskHandler<
    WorkspaceCommitPort,
    LeaseStorePort,
    TaskIdGeneratorPort,
    WorktreeProvisioner,
    ClockSource,
> {
    workspace_commit: WorkspaceCommitPort,
    lease_store: LeaseStorePort,
    task_id_generator: TaskIdGeneratorPort,
    worktree_provisioner: WorktreeProvisioner,
    /// Root of the project's Git repository, persisted into leases and rows.
    repository_root: PathBuf,
    work_dir: PathBuf,
    clock: ClockSource,
}

/// One scheduled worktree provisioning request, carrying the admitted run the
/// ownership commit binds to.
pub struct CreateScheduledWorktreeTaskRequest {
    pub project_id: String,
    pub title: String,
    pub base_branch: String,
    pub run_id: ScheduledRunId,
}

/// The committed task plus the owned worktree path, so the caller never needs a
/// separate workspace lookup before starting the run's session.
pub struct CreateScheduledWorktreeTaskResponse {
    pub task: Task,
    pub worktree_path: String,
}

impl<WorkspaceCommitPort, LeaseStorePort, TaskIdGeneratorPort, WorktreeProvisioner, ClockSource>
    CreateScheduledWorktreeTaskHandler<
        WorkspaceCommitPort,
        LeaseStorePort,
        TaskIdGeneratorPort,
        WorktreeProvisioner,
        ClockSource,
    >
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        workspace_commit: WorkspaceCommitPort,
        lease_store: LeaseStorePort,
        task_id_generator: TaskIdGeneratorPort,
        worktree_provisioner: WorktreeProvisioner,
        repository_root: PathBuf,
        work_dir: PathBuf,
        clock: ClockSource,
    ) -> Self {
        Self {
            workspace_commit,
            lease_store,
            task_id_generator,
            worktree_provisioner,
            repository_root,
            work_dir,
            clock,
        }
    }
}

impl<WorkspaceCommitPort, LeaseStorePort, TaskIdGeneratorPort, WorktreeProvisioner, ClockSource>
    CreateScheduledWorktreeTaskHandler<
        WorkspaceCommitPort,
        LeaseStorePort,
        TaskIdGeneratorPort,
        WorktreeProvisioner,
        ClockSource,
    >
where
    WorkspaceCommitPort: TaskWorkspaceCommit,
    LeaseStorePort: WorktreeProvisioningLeaseStore,
    TaskIdGeneratorPort: TaskIdGenerator,
    WorktreeProvisioner: TaskWorktreeProvisioner,
    ClockSource: Clock + Clone + Send + 'static,
{
    /// Provisions the worktree task and binds the run's ownership in the same
    /// commit, so no crash between provisioning and `Start` leaves the task
    /// without a durable owner.
    ///
    /// A `ScheduledRunNotAdmissible` error means the run was already settled by
    /// startup recovery, belongs to another project, or was bound concurrently;
    /// the lease has been handed to durable cleanup and the caller only fails
    /// the run through its regular transition, which the run row's state
    /// accepts or ignores when the run already reached a terminal state.
    pub fn handle(
        &self,
        request: CreateScheduledWorktreeTaskRequest,
    ) -> Result<CreateScheduledWorktreeTaskResponse, ApplicationError> {
        let provisioned = provision_worktree_task(
            &self.lease_store,
            &self.task_id_generator,
            &self.worktree_provisioner,
            &self.repository_root,
            &self.work_dir,
            &self.clock,
            ProjectId::new(request.project_id),
            request.title,
            request.base_branch,
            |task, worktree, lease_id, worktree_path| {
                self.workspace_commit
                    .commit_worktree_task_for_scheduled_run(
                        task,
                        worktree,
                        lease_id,
                        &ScheduledRunWorktreeLink {
                            run_id: request.run_id.clone(),
                            worktree_path: worktree_path.to_string(),
                        },
                    )
            },
        )?;
        Ok(CreateScheduledWorktreeTaskResponse {
            task: map_task(provisioned.task),
            worktree_path: provisioned.worktree_path,
        })
    }
}
