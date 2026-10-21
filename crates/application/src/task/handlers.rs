use crate::task::branch::{branch_name_for_workspace, workspace_branch_prefix};
use crate::task::mapper::map_task;
use crate::task::ports::{
    CreateTaskWorktreeRequest, TaskIdGenerator, TaskRepository, TaskWorktreeProvisioner,
};
use crate::task::provisioning::{
    PROVISIONING_LEASE_DURATION_MS, ProvisioningLeaseRenewal, TaskWorkspaceCommit,
    WorkspaceCommitOutcome, WorktreeProvisioningLeaseStore,
};
use crate::{ApplicationError, Clock, RepositoryError};
use ora_contracts::{
    CreateTaskRequest, CreateTaskResponse, GetTaskRequest, GetTaskResponse, ListTasksRequest,
    ListTasksResponse, UpdateTaskRequest, UpdateTaskResponse,
};
use ora_domain::{
    AuditFields, ProjectId, Task as DomainTask, TaskId, WorkspaceId, Worktree as DomainWorktree,
    WorktreeActivity as DomainWorktreeActivity, WorktreeProvisioningLease,
    WorktreeProvisioningLeaseId,
};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use uuid::Uuid;

const MAX_TASK_ID_GENERATION_ATTEMPTS: usize = 3;

/// One worktree-backed task that committed successfully, with the owned
/// worktree path that future recovery and retention paths resolve against.
pub(super) struct ProvisionedWorktreeTask {
    pub task: DomainTask,
    pub worktree_path: String,
}

/// Shared provisioning flow for worktree-backed tasks: identity selection, the
/// write-ahead lease, Git worktree creation, and one atomic commit through
/// `commit`.
///
/// Every non-committed outcome hands the lease to durable cleanup before
/// returning, so the provisioned Git resources always keep an owner: the lease
/// before the commit, the committed rows after it.
#[allow(clippy::too_many_arguments)]
pub(super) fn provision_worktree_task<
    LeaseStorePort,
    TaskIdGeneratorPort,
    WorktreeProvisioner,
    ClockSource,
    Commit,
>(
    lease_store: &LeaseStorePort,
    task_id_generator: &TaskIdGeneratorPort,
    worktree_provisioner: &WorktreeProvisioner,
    repository_root: &Path,
    work_dir: &Path,
    clock: &ClockSource,
    project_id: ProjectId,
    title: String,
    base_branch: String,
    commit: Commit,
) -> Result<ProvisionedWorktreeTask, ApplicationError>
where
    LeaseStorePort: WorktreeProvisioningLeaseStore,
    TaskIdGeneratorPort: TaskIdGenerator,
    WorktreeProvisioner: TaskWorktreeProvisioner,
    // Clone + Send + 'static keep the lease renewal thread available for slow
    // Git work, mirroring the handler-level composition requirements.
    ClockSource: Clock + Clone + Send + 'static,
    Commit: FnOnce(
        &DomainTask,
        &DomainWorktree,
        &WorktreeProvisioningLeaseId,
        &str,
    ) -> Result<WorkspaceCommitOutcome, RepositoryError>,
{
    let base_reference_name = base_branch.trim().to_owned();
    if base_reference_name.is_empty() {
        return Err(ApplicationError::TaskBaseBranchRequired);
    }
    worktree_provisioner
        .validate_repository()
        .map_err(ApplicationError::from_task_worktree_provisioner_error)?;
    let (task_id, workspace_id, branch_name, worktree_path) =
        select_available_task_workspace_identity(
            task_id_generator,
            worktree_provisioner,
            work_dir,
        )?;
    // Write-ahead lease: from here on the provisioned Git resources are
    // always owned by something durable — the lease, then the committed
    // rows — so no crash or lost race can orphan them.
    let now = clock.now_timestamp_millis();
    let lease = WorktreeProvisioningLease::new(
        WorktreeProvisioningLeaseId::new(Uuid::new_v4().to_string()),
        project_id.clone(),
        workspace_id.clone(),
        repository_root.to_string_lossy().into_owned(),
        worktree_path.to_string_lossy().into_owned(),
        branch_name.clone(),
        now + PROVISIONING_LEASE_DURATION_MS,
        now,
    );
    lease_store
        .create_lease(&lease)
        .map_err(ApplicationError::from_worktree_repository_error)?;
    let renewal = ProvisioningLeaseRenewal::spawn(lease_store.clone(), lease.id.clone(), {
        let clock = clock.clone();
        move || clock.now_timestamp_millis()
    });

    let provisioned_worktree =
        match worktree_provisioner.create_task_worktree(CreateTaskWorktreeRequest {
            branch_name: branch_name.clone(),
            base_reference_name,
            worktree_path,
        }) {
            Ok(provisioned) => provisioned,
            Err(error) => {
                drop(renewal);
                release_lease_to_cleanup(lease_store, clock, &lease.id);
                return Err(ApplicationError::from_task_worktree_provisioner_error(
                    error,
                ));
            }
        };

    let now = clock.now_timestamp_millis();
    let baseline = match ora_domain::WorktreeBaseline::recorded(provisioned_worktree.base_commit_id)
    {
        Ok(baseline) => baseline,
        Err(error) => {
            drop(renewal);
            release_lease_to_cleanup(lease_store, clock, &lease.id);
            return Err(ApplicationError::TaskWorktreeProvisioner {
                source: crate::TaskWorktreeProvisionerError::operation_failed(
                    "failed to record task worktree baseline",
                    error,
                ),
            });
        }
    };
    let worktree = DomainWorktree::new(
        workspace_id.clone(),
        Some(branch_name),
        baseline,
        DomainWorktreeActivity::Active,
        AuditFields::new(now, now, false),
    );
    let task = DomainTask::new(
        task_id,
        project_id.clone(),
        workspace_id,
        title,
        AuditFields::new(now, now, false),
    );

    let worktree_path_string = lease.checkout_root.clone();
    let committed = commit(&task, &worktree, &lease.id, &worktree_path_string);
    drop(renewal);
    match committed {
        Ok(WorkspaceCommitOutcome::Committed) => Ok(ProvisionedWorktreeTask {
            task,
            worktree_path: worktree_path_string,
        }),
        Ok(WorkspaceCommitOutcome::ProjectNotVisible) => {
            release_lease_to_cleanup(lease_store, clock, &lease.id);
            Err(ApplicationError::ProjectNotFound {
                project_id: project_id.to_string(),
            })
        }
        Ok(WorkspaceCommitOutcome::RunNotAdmissible) => {
            release_lease_to_cleanup(lease_store, clock, &lease.id);
            Err(ApplicationError::ScheduledRunNotAdmissible)
        }
        Err(error) => {
            release_lease_to_cleanup(lease_store, clock, &lease.id);
            Err(ApplicationError::from_task_repository_error(error))
        }
    }
}

/// Hands the lease's Git resources to the durable cleanup path.
///
/// Failure is deliberately tolerated: the lease then simply expires and the
/// cleanup worker reclaims it on schedule.
pub(super) fn release_lease_to_cleanup<LeaseStorePort, ClockSource>(
    lease_store: &LeaseStorePort,
    clock: &ClockSource,
    lease_id: &WorktreeProvisioningLeaseId,
) where
    LeaseStorePort: WorktreeProvisioningLeaseStore,
    ClockSource: Clock,
{
    let _ = lease_store.release_to_cleanup(lease_id, clock.now_timestamp_millis());
}

/// Generates independent Task and Workspace identities without colliding with Git resources.
pub(super) fn select_available_task_workspace_identity<TaskIdGeneratorPort, WorktreeProvisioner>(
    task_id_generator: &TaskIdGeneratorPort,
    worktree_provisioner: &WorktreeProvisioner,
    work_dir: &Path,
) -> Result<(TaskId, WorkspaceId, String, PathBuf), ApplicationError>
where
    TaskIdGeneratorPort: TaskIdGenerator,
    WorktreeProvisioner: TaskWorktreeProvisioner,
{
    for _ in 0..MAX_TASK_ID_GENERATION_ATTEMPTS {
        let task_id = task_id_generator.generate_task_id();
        let workspace_id = WorkspaceId::new(Uuid::new_v4().to_string());
        let branch_prefix = workspace_branch_prefix(&workspace_id);

        if workspace_branch_prefix_exists_in_work_dir(work_dir, &branch_prefix)? {
            continue;
        }

        let branch_name = branch_name_for_workspace(&workspace_id);
        let branch_exists = worktree_provisioner
            .task_branch_exists(&branch_name)
            .map_err(ApplicationError::from_task_worktree_provisioner_error)?;
        if !branch_exists {
            return Ok((
                task_id,
                workspace_id.clone(),
                branch_name,
                worktree_path_for_workspace(work_dir, &workspace_id),
            ));
        }
    }

    Err(ApplicationError::TaskWorkspaceIdExhausted {
        attempts: MAX_TASK_ID_GENERATION_ATTEMPTS,
    })
}

/// Handles task creation without depending on transport-specific concerns.
pub struct CreateTaskHandler<
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

impl<WorkspaceCommitPort, LeaseStorePort, TaskIdGeneratorPort, WorktreeProvisioner, ClockSource>
    CreateTaskHandler<
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
    CreateTaskHandler<
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
    /// Creates a task together with its isolated linked Git worktree.
    pub fn handle(
        &self,
        request: CreateTaskRequest,
    ) -> Result<CreateTaskResponse, ApplicationError> {
        self.create_worktree_task(request)
    }

    /// Provisions a linked worktree before persisting the task that owns it.
    fn create_worktree_task(
        &self,
        request: CreateTaskRequest,
    ) -> Result<CreateTaskResponse, ApplicationError> {
        let base_branch = request
            .base_branch
            .as_deref()
            .map(str::trim)
            .filter(|branch| !branch.is_empty())
            .ok_or(ApplicationError::TaskBaseBranchRequired)?
            .to_string();
        let provisioned = provision_worktree_task(
            &self.lease_store,
            &self.task_id_generator,
            &self.worktree_provisioner,
            &self.repository_root,
            &self.work_dir,
            &self.clock,
            ProjectId::new(request.project_id.clone()),
            request.title,
            base_branch,
            |task, worktree, lease_id, _worktree_path| {
                self.workspace_commit
                    .commit_worktree_task(task, worktree, lease_id)
            },
        )?;
        Ok(CreateTaskResponse {
            task: map_task(provisioned.task),
        })
    }
}

/// Handles one task lookup without depending on transport-specific concerns.
pub struct GetTaskHandler<Repository> {
    repository: Repository,
}

impl<Repository> GetTaskHandler<Repository> {
    pub fn new(repository: Repository) -> Self {
        Self { repository }
    }
}

impl<Repository> GetTaskHandler<Repository>
where
    Repository: TaskRepository,
{
    /// Loads one visible task or returns a stable not-found application error.
    pub fn handle(&self, request: GetTaskRequest) -> Result<GetTaskResponse, ApplicationError> {
        let task_id = TaskId::new(request.task_id);
        let task = self
            .repository
            .find_task(&task_id)
            .map_err(ApplicationError::from_task_repository_error)?;

        match task {
            Some(task) => Ok(GetTaskResponse {
                task: map_task(task),
            }),
            None => {
                let error = ApplicationError::TaskNotFound {
                    task_id: task_id.to_string(),
                };
                Err(error)
            }
        }
    }
}

/// Handles task listing without depending on transport-specific concerns.
pub struct ListTasksHandler<Repository> {
    repository: Repository,
}

impl<Repository> ListTasksHandler<Repository> {
    pub fn new(repository: Repository) -> Self {
        Self { repository }
    }
}

impl<Repository> ListTasksHandler<Repository>
where
    Repository: TaskRepository,
{
    /// Lists every visible task and maps each one into the shared contract view.
    pub fn handle(
        &self,
        _request: ListTasksRequest,
    ) -> Result<ListTasksResponse, ApplicationError> {
        let tasks = self
            .repository
            .list_tasks()
            .map_err(ApplicationError::from_task_repository_error)?;

        Ok(ListTasksResponse {
            tasks: tasks.into_iter().map(map_task).collect(),
        })
    }
}

/// Handles task updates without depending on transport-specific concerns.
pub struct UpdateTaskHandler<Repository, ClockSource> {
    repository: Repository,
    clock: ClockSource,
}

impl<Repository, ClockSource> UpdateTaskHandler<Repository, ClockSource> {
    pub fn new(repository: Repository, clock: ClockSource) -> Self {
        Self { repository, clock }
    }
}

impl<Repository, ClockSource> UpdateTaskHandler<Repository, ClockSource>
where
    Repository: TaskRepository,
    ClockSource: Clock,
{
    /// Replaces the public task fields while preserving persistence-managed audit state.
    pub fn handle(
        &self,
        request: UpdateTaskRequest,
    ) -> Result<UpdateTaskResponse, ApplicationError> {
        let task_id = TaskId::new(request.task_id);
        let existing_task = self
            .repository
            .find_task(&task_id)
            .map_err(ApplicationError::from_task_repository_error)?;

        let existing_task = match existing_task {
            Some(existing_task) => existing_task,
            None => {
                let error = ApplicationError::TaskNotFound {
                    task_id: task_id.to_string(),
                };
                return Err(error);
            }
        };

        let task = DomainTask {
            id: task_id,
            project_id: existing_task.project_id,
            title: request.title,
            workspace_id: existing_task.workspace_id,
            audit_fields: AuditFields::new(
                existing_task.audit_fields.created_at,
                self.clock.now_timestamp_millis(),
                existing_task.audit_fields.is_deleted,
            ),
        };
        let task = self
            .repository
            .update_task(task)
            .map_err(ApplicationError::from_task_repository_error)?;

        Ok(UpdateTaskResponse {
            task: map_task(task),
        })
    }
}

/// Derives the owned linked-worktree path from the configured root and Workspace identity.
fn worktree_path_for_workspace(work_dir: &Path, workspace_id: &WorkspaceId) -> PathBuf {
    work_dir.join(workspace_id.to_string())
}

/// Checks existing worktree folders before branch creation because branch names use short ids.
fn workspace_branch_prefix_exists_in_work_dir(
    work_dir: &Path,
    branch_prefix: &str,
) -> Result<bool, ApplicationError> {
    let entries = match fs::read_dir(work_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(source) => {
            return Err(ApplicationError::TaskFilesystem {
                context: "failed to inspect task worktree directory",
                source,
            });
        }
    };

    for entry in entries {
        let entry = entry.map_err(|source| ApplicationError::TaskFilesystem {
            context: "failed to inspect task worktree directory",
            source,
        })?;
        let file_type = entry
            .file_type()
            .map_err(|source| ApplicationError::TaskFilesystem {
                context: "failed to inspect task worktree directory",
                source,
            })?;

        if file_type.is_dir()
            && entry
                .file_name()
                .to_string_lossy()
                .starts_with(branch_prefix)
        {
            return Ok(true);
        }
    }

    Ok(false)
}

#[cfg(test)]
mod workspace_branch_prefix_tests {
    use super::workspace_branch_prefix_exists_in_work_dir;
    use pretty_assertions::assert_eq;
    use std::fs;
    use std::path::PathBuf;

    /// Verifies an absent worktree root is treated as the first task creation, not an inspection failure.
    #[test]
    fn reports_no_collision_when_work_dir_does_not_exist() {
        let work_dir = unique_test_work_dir("missing");

        assert_eq!(
            workspace_branch_prefix_exists_in_work_dir(&work_dir, "12345678"),
            Ok(false)
        );
    }

    /// Verifies only directories with the requested prefix reserve a task branch prefix.
    #[test]
    fn detects_matching_directory_prefixes() {
        let work_dir = unique_test_work_dir("matching-directory");
        fs::create_dir_all(work_dir.join("12345678-existing"))
            .unwrap_or_else(|error| panic!("failed to create collision fixture: {error}"));

        assert_eq!(
            workspace_branch_prefix_exists_in_work_dir(&work_dir, "12345678"),
            Ok(true)
        );

        fs::remove_dir_all(&work_dir)
            .unwrap_or_else(|error| panic!("failed to remove collision fixture: {error}"));
    }

    /// Verifies ordinary files and unrelated directories do not reserve a task branch prefix.
    #[test]
    fn ignores_files_and_unrelated_directories() {
        let work_dir = unique_test_work_dir("unrelated-entries");
        fs::create_dir_all(work_dir.join("87654321-existing"))
            .unwrap_or_else(|error| panic!("failed to create unrelated directory: {error}"));
        fs::write(work_dir.join("12345678-file"), b"not a worktree")
            .unwrap_or_else(|error| panic!("failed to create ordinary file: {error}"));

        assert_eq!(
            workspace_branch_prefix_exists_in_work_dir(&work_dir, "12345678"),
            Ok(false)
        );

        fs::remove_dir_all(&work_dir)
            .unwrap_or_else(|error| panic!("failed to remove unrelated entries: {error}"));
    }

    /// Builds an isolated filesystem location for branch-prefix unit tests.
    fn unique_test_work_dir(name: &str) -> PathBuf {
        let work_dir = std::env::temp_dir().join(format!(
            "ora-application-prefix-unit-{name}-{}",
            std::process::id()
        ));
        if work_dir.exists() {
            fs::remove_dir_all(&work_dir)
                .unwrap_or_else(|error| panic!("failed to reset unit-test work dir: {error}"));
        }
        work_dir
    }
}
