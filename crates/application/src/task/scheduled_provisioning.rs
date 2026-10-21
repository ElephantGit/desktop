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
#[derive(Debug)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::ports::{
        CreateTaskWorktreeRequest, DeleteTaskWorktreeRequest, TaskWorktreeDeletionMode,
        TaskWorktreeProvisioner, TaskWorktreeProvisionerError,
    };
    use crate::task::provisioning::WorkspaceCommitOutcome;
    use crate::{ApplicationError, Clock, RepositoryError};
    use ora_domain::{
        Task as DomainTask, TaskId, Worktree as DomainWorktree, WorktreeProvisioningLease,
        WorktreeProvisioningLeaseId,
    };
    use pretty_assertions::assert_eq;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct FixedClock;

    impl Clock for FixedClock {
        fn now_timestamp_millis(&self) -> i64 {
            1_000
        }
    }

    struct FixedTaskIds;

    impl crate::task::ports::TaskIdGenerator for FixedTaskIds {
        fn generate_task_id(&self) -> TaskId {
            TaskId::new("task-new")
        }
    }

    /// Records released leases so tests can assert the compensation handoff.
    #[derive(Clone, Default)]
    struct RecordingLeaseStore {
        released: Arc<Mutex<Vec<String>>>,
    }

    impl RecordingLeaseStore {
        fn released(&self) -> Vec<String> {
            self.released.lock().expect("lease records").clone()
        }
    }

    impl crate::task::provisioning::WorktreeProvisioningLeaseStore for RecordingLeaseStore {
        fn create_lease(&self, _lease: &WorktreeProvisioningLease) -> Result<(), RepositoryError> {
            Ok(())
        }

        fn renew_lease(
            &self,
            _lease_id: &WorktreeProvisioningLeaseId,
            _lease_expires_at: i64,
            _now: i64,
        ) -> Result<bool, RepositoryError> {
            Ok(true)
        }

        fn release_to_cleanup(
            &self,
            lease_id: &WorktreeProvisioningLeaseId,
            _now: i64,
        ) -> Result<(), RepositoryError> {
            self.released
                .lock()
                .expect("lease records")
                .push(lease_id.as_ref().to_string());
            Ok(())
        }
    }

    struct StubProvisioner {
        fail_git: bool,
    }

    impl TaskWorktreeProvisioner for StubProvisioner {
        fn validate_repository(&self) -> Result<(), TaskWorktreeProvisionerError> {
            Ok(())
        }

        fn task_branch_exists(
            &self,
            _branch_name: &str,
        ) -> Result<bool, TaskWorktreeProvisionerError> {
            Ok(false)
        }

        fn create_task_worktree(
            &self,
            _request: CreateTaskWorktreeRequest,
        ) -> Result<crate::task::ports::CreateTaskWorktreeResponse, TaskWorktreeProvisionerError>
        {
            if self.fail_git {
                return Err(TaskWorktreeProvisionerError::NotARepository);
            }
            Ok(crate::task::ports::CreateTaskWorktreeResponse {
                base_commit_id: "base-commit".to_string(),
            })
        }

        fn delete_task_worktree(
            &self,
            _request: DeleteTaskWorktreeRequest,
        ) -> Result<(), TaskWorktreeProvisionerError> {
            Ok(())
        }
    }

    /// Records the ownership link the handler passed to the commit and replays a
    /// configured outcome or failure.
    #[derive(Clone)]
    struct StubCommit {
        captured: Arc<Mutex<Vec<(String, String, String)>>>,
        outcome: Option<WorkspaceCommitOutcome>,
        error: Option<String>,
    }

    impl StubCommit {
        fn committed(outcome: WorkspaceCommitOutcome) -> Self {
            Self {
                captured: Arc::new(Mutex::new(Vec::new())),
                outcome: Some(outcome),
                error: None,
            }
        }

        fn failing(error: &str) -> Self {
            Self {
                captured: Arc::new(Mutex::new(Vec::new())),
                outcome: None,
                error: Some(error.to_string()),
            }
        }

        fn captured(&self) -> Vec<(String, String, String)> {
            self.captured.lock().expect("commit records").clone()
        }
    }

    impl TaskWorkspaceCommit for StubCommit {
        fn commit_worktree_task(
            &self,
            _task: &DomainTask,
            _worktree: &DomainWorktree,
            _lease_id: &WorktreeProvisioningLeaseId,
        ) -> Result<WorkspaceCommitOutcome, RepositoryError> {
            panic!("the scheduled handler must not use the plain commit path")
        }

        fn commit_worktree_task_for_scheduled_run(
            &self,
            task: &DomainTask,
            _worktree: &DomainWorktree,
            _lease_id: &WorktreeProvisioningLeaseId,
            run_link: &ScheduledRunWorktreeLink,
        ) -> Result<WorkspaceCommitOutcome, RepositoryError> {
            self.captured.lock().expect("commit records").push((
                task.id.as_ref().to_string(),
                run_link.run_id.as_ref().to_string(),
                run_link.worktree_path.clone(),
            ));
            if let Some(error) = &self.error {
                return Err(RepositoryError::from_message(error.clone()));
            }
            Ok(self.outcome.expect("configured outcome"))
        }
    }

    fn handler(
        commit: StubCommit,
        lease_store: RecordingLeaseStore,
        provisioner: StubProvisioner,
    ) -> CreateScheduledWorktreeTaskHandler<
        StubCommit,
        RecordingLeaseStore,
        FixedTaskIds,
        StubProvisioner,
        FixedClock,
    > {
        CreateScheduledWorktreeTaskHandler::new(
            commit,
            lease_store,
            FixedTaskIds,
            provisioner,
            PathBuf::from("repository-root"),
            PathBuf::from("work-dir"),
            FixedClock,
        )
    }

    fn request() -> CreateScheduledWorktreeTaskRequest {
        CreateScheduledWorktreeTaskRequest {
            project_id: "project-1".to_string(),
            title: "Scheduled run run-1".to_string(),
            base_branch: "main".to_string(),
            run_id: ScheduledRunId::new("run-1"),
        }
    }

    #[test]
    fn ownership_commit_binds_the_run_and_leaves_no_lease_behind() {
        let commit = StubCommit::committed(WorkspaceCommitOutcome::Committed);
        let lease_store = RecordingLeaseStore::default();
        let response = handler(
            commit.clone(),
            lease_store.clone(),
            StubProvisioner { fail_git: false },
        )
        .handle(request())
        .expect("committed provisioning");

        // The handler passes the admitted run and the owned worktree path into
        // the commit; the lease is removed by that same commit, so nothing is
        // released to cleanup here.
        let captured = commit.captured();
        assert_eq!(
            captured,
            vec![(
                "task-new".to_string(),
                "run-1".to_string(),
                response.worktree_path.clone()
            )]
        );
        assert!(response.worktree_path.starts_with("work-dir"));
        assert_eq!(response.task.id, "task-new");
        assert_eq!(lease_store.released(), Vec::<String>::new());
    }

    #[test]
    fn inadmissible_run_releases_the_lease_and_reports_the_error() {
        let commit = StubCommit::committed(WorkspaceCommitOutcome::RunNotAdmissible);
        let lease_store = RecordingLeaseStore::default();
        let error = handler(
            commit,
            lease_store.clone(),
            StubProvisioner { fail_git: false },
        )
        .handle(request())
        .expect_err("inadmissible run");

        assert!(matches!(error, ApplicationError::ScheduledRunNotAdmissible));
        // The run was settled or bound concurrently: the freshly provisioned
        // resources go back to the lease's durable cleanup path.
        assert_eq!(lease_store.released().len(), 1);
    }

    #[test]
    fn commit_failure_releases_the_lease_to_compensation() {
        // Injects the lease-delete failure boundary: a commit that rolls back
        // must leave the lease in charge and never surface a task.
        let commit = StubCommit::failing("lease delete failed");
        let lease_store = RecordingLeaseStore::default();
        let error = handler(
            commit,
            lease_store.clone(),
            StubProvisioner { fail_git: false },
        )
        .handle(request())
        .expect_err("failed commit");

        assert!(matches!(error, ApplicationError::TaskRepository { .. }));
        assert_eq!(lease_store.released().len(), 1);
    }

    #[test]
    fn git_failure_after_the_lease_hands_resources_to_durable_cleanup() {
        let commit = StubCommit::committed(WorkspaceCommitOutcome::Committed);
        let lease_store = RecordingLeaseStore::default();
        let error = handler(
            commit,
            lease_store.clone(),
            StubProvisioner { fail_git: true },
        )
        .handle(request())
        .expect_err("failed git provisioning");

        assert!(matches!(
            error,
            ApplicationError::TaskWorktreeRequiresGitRepository
        ));
        // The write-ahead lease is handed to durable cleanup: the resources
        // never outlive an owner (AC1).
        assert_eq!(lease_store.released().len(), 1);
    }
}
