use std::sync::Arc;

use ora_contracts::{
    CreateScheduledTaskRequest, CreateScheduledTaskResponse, DeleteScheduledTaskRequest,
    DeleteScheduledTaskResponse, GetScheduledTaskRequest, GetScheduledTaskResponse,
    ListScheduledTaskHistoryRequest, ListScheduledTaskHistoryResponse, ListScheduledTasksRequest,
    ListScheduledTasksResponse, RevokeScheduledTaskApprovalRequest,
    RevokeScheduledTaskApprovalResponse, ScheduledTaskExecutionLocation as ContractLocation,
    UpdateScheduledTaskRequest, UpdateScheduledTaskResponse,
};
use ora_domain::{
    AgentRef, AuditFields, ProjectId, ScheduledTask, ScheduledTaskExecutionLocation,
    ScheduledTaskId, WorkspaceLocation,
};

use super::mapper::{approval_from_policy, map_history_entry, map_task, parse_schedule};
use super::{
    ScheduledTaskHistoryRepository, ScheduledTaskIdGenerator, ScheduledTaskRepository,
    ScheduledTaskTargetValidator,
};
use crate::{
    ApplicationError, Clock, ProjectRepository, TaskWorktreeProvisionerError, WorkspaceRepository,
};

/// Handles creation of a first-release local scheduled-task definition.
pub struct CreateScheduledTaskHandler<
    Repository,
    Projects,
    Workspaces,
    TargetValidator,
    IdGenerator,
    ClockSource,
> {
    repository: Arc<Repository>,
    projects: Arc<Projects>,
    workspaces: Arc<Workspaces>,
    target_validator: TargetValidator,
    id_generator: IdGenerator,
    clock: ClockSource,
}

impl<Repository, Projects, Workspaces, TargetValidator, IdGenerator, ClockSource>
    CreateScheduledTaskHandler<
        Repository,
        Projects,
        Workspaces,
        TargetValidator,
        IdGenerator,
        ClockSource,
    >
{
    pub fn new(
        repository: Arc<Repository>,
        projects: Arc<Projects>,
        workspaces: Arc<Workspaces>,
        target_validator: TargetValidator,
        id_generator: IdGenerator,
        clock: ClockSource,
    ) -> Self {
        Self {
            repository,
            projects,
            workspaces,
            target_validator,
            id_generator,
            clock,
        }
    }
}

impl<Repository, Projects, Workspaces, TargetValidator, IdGenerator, ClockSource>
    CreateScheduledTaskHandler<
        Repository,
        Projects,
        Workspaces,
        TargetValidator,
        IdGenerator,
        ClockSource,
    >
where
    Repository: ScheduledTaskRepository + Send + Sync + 'static,
    Projects: ProjectRepository + Send + Sync + 'static,
    Workspaces: WorkspaceRepository + Send + Sync + 'static,
    TargetValidator: ScheduledTaskTargetValidator,
    IdGenerator: ScheduledTaskIdGenerator,
    ClockSource: Clock,
{
    /// Creates one local task after project and unattended-consent validation.
    pub fn handle(
        &self,
        request: CreateScheduledTaskRequest,
    ) -> Result<CreateScheduledTaskResponse, ApplicationError> {
        let task = self.prepare(request)?;
        self.persist(task)
    }

    /// Validates and freezes a definition without writing it, so adapters can check live runtime
    /// eligibility before committing the task.
    pub fn prepare(
        &self,
        request: CreateScheduledTaskRequest,
    ) -> Result<ScheduledTask, ApplicationError> {
        if request.execution_location == ContractLocation::Cloud {
            return Err(ApplicationError::ScheduledTaskCloudUnavailable);
        }

        let now = self.clock.now_timestamp_millis();
        let project_id = ProjectId::new(request.project_id);
        require_git_project(self.projects.as_ref(), &project_id)?;
        let approval = request
            .approval_policy
            .map(|policy| approval_from_policy(policy, now));
        let task = ScheduledTask::new(
            self.id_generator.generate_scheduled_task_id(),
            request.name,
            request.enabled,
            parse_schedule(request.schedule)?,
            ScheduledTaskExecutionLocation::Local,
            project_id,
            AgentRef::parse(request.agent_ref)
                .map_err(ApplicationError::from_scheduled_task_domain_error)?,
            request.model,
            request.prompt,
            approval,
            AuditFields::new(now, now, /*is_deleted*/ false),
        )
        .map_err(ApplicationError::from_scheduled_task_domain_error)?;
        require_activation_approval(&task)?;
        require_active_local_target(&task, self.workspaces.as_ref(), &self.target_validator)?;

        Ok(task)
    }

    /// Persists a definition returned by [`Self::prepare`].
    pub fn persist(
        &self,
        task: ScheduledTask,
    ) -> Result<CreateScheduledTaskResponse, ApplicationError> {
        let task = self
            .repository
            .create_scheduled_task(task)
            .map_err(ApplicationError::from_scheduled_task_repository_error)?;
        Ok(CreateScheduledTaskResponse {
            task: map_task(task),
        })
    }
}

/// Handles lookup of one visible scheduled-task definition.
pub struct GetScheduledTaskHandler<Repository> {
    repository: Arc<Repository>,
}

impl<Repository> GetScheduledTaskHandler<Repository> {
    pub fn new(repository: Arc<Repository>) -> Self {
        Self { repository }
    }
}

impl<Repository> GetScheduledTaskHandler<Repository>
where
    Repository: ScheduledTaskRepository + Send + Sync + 'static,
{
    pub fn handle(
        &self,
        request: GetScheduledTaskRequest,
    ) -> Result<GetScheduledTaskResponse, ApplicationError> {
        let task_id = ScheduledTaskId::new(request.task_id);
        let task = find_task(self.repository.as_ref(), &task_id)?;
        Ok(GetScheduledTaskResponse {
            task: map_task(task),
        })
    }
}

/// Handles deterministic listing of visible scheduled-task definitions.
pub struct ListScheduledTasksHandler<Repository> {
    repository: Arc<Repository>,
}

impl<Repository> ListScheduledTasksHandler<Repository> {
    pub fn new(repository: Arc<Repository>) -> Self {
        Self { repository }
    }
}

impl<Repository> ListScheduledTasksHandler<Repository>
where
    Repository: ScheduledTaskRepository + Send + Sync + 'static,
{
    pub fn handle(
        &self,
        _request: ListScheduledTasksRequest,
    ) -> Result<ListScheduledTasksResponse, ApplicationError> {
        let tasks = self
            .repository
            .list_scheduled_tasks()
            .map_err(ApplicationError::from_scheduled_task_repository_error)?;
        Ok(ListScheduledTasksResponse {
            tasks: tasks.into_iter().map(map_task).collect(),
        })
    }
}

/// Handles newest-first reads of persisted run and missed facts.
pub struct ListScheduledTaskHistoryHandler<Repository> {
    repository: Arc<Repository>,
}

impl<Repository> ListScheduledTaskHistoryHandler<Repository> {
    pub fn new(repository: Arc<Repository>) -> Self {
        Self { repository }
    }
}

impl<Repository> ListScheduledTaskHistoryHandler<Repository>
where
    Repository: ScheduledTaskHistoryRepository + Send + Sync + 'static,
{
    pub fn handle(
        &self,
        request: ListScheduledTaskHistoryRequest,
    ) -> Result<ListScheduledTaskHistoryResponse, ApplicationError> {
        let entries = self
            .repository
            .list_scheduled_task_history(&ScheduledTaskId::new(request.task_id))
            .map_err(ApplicationError::from_scheduled_task_repository_error)?;
        Ok(ListScheduledTaskHistoryResponse {
            entries: entries.into_iter().map(map_history_entry).collect(),
        })
    }
}

/// Handles replacement of editable task fields while preserving frozen ownership rules.
pub struct UpdateScheduledTaskHandler<
    Repository,
    Projects,
    Workspaces,
    TargetValidator,
    ClockSource,
> {
    repository: Arc<Repository>,
    projects: Arc<Projects>,
    workspaces: Arc<Workspaces>,
    target_validator: TargetValidator,
    clock: ClockSource,
}

impl<Repository, Projects, Workspaces, TargetValidator, ClockSource>
    UpdateScheduledTaskHandler<Repository, Projects, Workspaces, TargetValidator, ClockSource>
{
    pub fn new(
        repository: Arc<Repository>,
        projects: Arc<Projects>,
        workspaces: Arc<Workspaces>,
        target_validator: TargetValidator,
        clock: ClockSource,
    ) -> Self {
        Self {
            repository,
            projects,
            workspaces,
            target_validator,
            clock,
        }
    }
}

impl<Repository, Projects, Workspaces, TargetValidator, ClockSource>
    UpdateScheduledTaskHandler<Repository, Projects, Workspaces, TargetValidator, ClockSource>
where
    Repository: ScheduledTaskRepository + Send + Sync + 'static,
    Projects: ProjectRepository + Send + Sync + 'static,
    Workspaces: WorkspaceRepository + Send + Sync + 'static,
    TargetValidator: ScheduledTaskTargetValidator,
    ClockSource: Clock,
{
    /// Replaces editable fields, increments frozen Prompt revision, and revalidates consent.
    pub fn handle(
        &self,
        request: UpdateScheduledTaskRequest,
    ) -> Result<UpdateScheduledTaskResponse, ApplicationError> {
        let replacement = self.prepare(request)?;
        self.persist(replacement)
    }

    /// Validates and freezes an update without writing it, preserving the existing revision rules.
    pub fn prepare(
        &self,
        request: UpdateScheduledTaskRequest,
    ) -> Result<ScheduledTask, ApplicationError> {
        let task_id = ScheduledTaskId::new(request.task_id);
        let existing = find_task(self.repository.as_ref(), &task_id)?;
        if existing.execution_location == ScheduledTaskExecutionLocation::Cloud {
            return Err(ApplicationError::ScheduledTaskCloudUnavailable);
        }

        let now = self.clock.now_timestamp_millis();
        let project_id = ProjectId::new(request.project_id);
        require_git_project(self.projects.as_ref(), &project_id)?;
        let project_changed = project_id != existing.project_id;
        let schedule = parse_schedule(request.schedule)?;
        let agent_ref = AgentRef::parse(request.agent_ref)
            .map_err(ApplicationError::from_scheduled_task_domain_error)?;
        let prompt = request.prompt.trim().to_string();
        let revision_changed = schedule != existing.schedule
            || project_changed
            || agent_ref != existing.agent_ref
            || request.model != existing.model
            || prompt != existing.prompt;
        let approval = match request.approval_policy {
            Some(policy) => Some(approval_from_policy(policy, now)),
            None if project_changed => None,
            None => existing.approval.clone(),
        };
        let mut replacement = ScheduledTask::new(
            task_id,
            request.name,
            request.enabled,
            schedule,
            existing.execution_location,
            project_id,
            agent_ref,
            request.model,
            prompt,
            approval,
            AuditFields::new(
                existing.audit_fields.created_at,
                now,
                /*is_deleted*/ false,
            ),
        )
        .map_err(ApplicationError::from_scheduled_task_domain_error)?;
        replacement.revision = existing.revision + u64::from(revision_changed);
        require_activation_approval(&replacement)?;
        require_active_local_target(
            &replacement,
            self.workspaces.as_ref(),
            &self.target_validator,
        )?;

        Ok(replacement)
    }

    /// Persists a replacement returned by [`Self::prepare`].
    pub fn persist(
        &self,
        replacement: ScheduledTask,
    ) -> Result<UpdateScheduledTaskResponse, ApplicationError> {
        let task_id = replacement.id.clone();
        let task = self
            .repository
            .update_scheduled_task(replacement)
            .map_err(ApplicationError::from_scheduled_task_repository_error)?
            .ok_or_else(|| ApplicationError::ScheduledTaskNotFound {
                task_id: task_id.to_string(),
            })?;
        Ok(UpdateScheduledTaskResponse {
            task: map_task(task),
        })
    }
}

/// Handles soft deletion of one task definition while retaining run history.
pub struct DeleteScheduledTaskHandler<Repository, ClockSource> {
    repository: Arc<Repository>,
    clock: ClockSource,
}

impl<Repository, ClockSource> DeleteScheduledTaskHandler<Repository, ClockSource> {
    pub fn new(repository: Arc<Repository>, clock: ClockSource) -> Self {
        Self { repository, clock }
    }
}

impl<Repository, ClockSource> DeleteScheduledTaskHandler<Repository, ClockSource>
where
    Repository: ScheduledTaskRepository + Send + Sync + 'static,
    ClockSource: Clock,
{
    pub fn handle(
        &self,
        request: DeleteScheduledTaskRequest,
    ) -> Result<DeleteScheduledTaskResponse, ApplicationError> {
        let task_id = ScheduledTaskId::new(request.task_id);
        let deleted = self
            .repository
            .soft_delete_scheduled_task(&task_id, self.clock.now_timestamp_millis())
            .map_err(ApplicationError::from_scheduled_task_repository_error)?;
        if !deleted {
            return Err(ApplicationError::ScheduledTaskNotFound {
                task_id: task_id.to_string(),
            });
        }
        Ok(DeleteScheduledTaskResponse {
            task_id: task_id.to_string(),
        })
    }
}

/// Handles explicit unattended-consent revocation and immediate task disablement.
pub struct RevokeScheduledTaskApprovalHandler<Repository, ClockSource> {
    repository: Arc<Repository>,
    clock: ClockSource,
}

impl<Repository, ClockSource> RevokeScheduledTaskApprovalHandler<Repository, ClockSource> {
    pub fn new(repository: Arc<Repository>, clock: ClockSource) -> Self {
        Self { repository, clock }
    }
}

impl<Repository, ClockSource> RevokeScheduledTaskApprovalHandler<Repository, ClockSource>
where
    Repository: ScheduledTaskRepository + Send + Sync + 'static,
    ClockSource: Clock,
{
    pub fn handle(
        &self,
        request: RevokeScheduledTaskApprovalRequest,
    ) -> Result<RevokeScheduledTaskApprovalResponse, ApplicationError> {
        let task_id = ScheduledTaskId::new(request.task_id);
        let mut task = find_task(self.repository.as_ref(), &task_id)?;
        task.enabled = false;
        task.approval = None;
        task.audit_fields.updated_at = self.clock.now_timestamp_millis();
        let task = self
            .repository
            .update_scheduled_task(task)
            .map_err(ApplicationError::from_scheduled_task_repository_error)?
            .ok_or_else(|| ApplicationError::ScheduledTaskNotFound {
                task_id: task_id.to_string(),
            })?;
        Ok(RevokeScheduledTaskApprovalResponse {
            task: map_task(task),
        })
    }
}

fn find_task<Repository>(
    repository: &Repository,
    task_id: &ScheduledTaskId,
) -> Result<ScheduledTask, ApplicationError>
where
    Repository: ScheduledTaskRepository,
{
    repository
        .find_scheduled_task(task_id)
        .map_err(ApplicationError::from_scheduled_task_repository_error)?
        .ok_or_else(|| ApplicationError::ScheduledTaskNotFound {
            task_id: task_id.to_string(),
        })
}

fn require_git_project<Projects>(
    projects: &Projects,
    project_id: &ProjectId,
) -> Result<(), ApplicationError>
where
    Projects: ProjectRepository,
{
    let project = projects
        .find_project(project_id)
        .map_err(ApplicationError::from_project_repository_error)?
        .ok_or_else(|| ApplicationError::ProjectNotFound {
            project_id: project_id.to_string(),
        })?;
    if project.repository_kind != "git" {
        return Err(ApplicationError::ScheduledTaskRequiresGitProject);
    }
    Ok(())
}

fn require_activation_approval(task: &ScheduledTask) -> Result<(), ApplicationError> {
    if task.enabled && !task.local_activation_allowed() {
        return Err(ApplicationError::ScheduledTaskApprovalRequired);
    }
    Ok(())
}

fn require_active_local_target<Workspaces, TargetValidator>(
    task: &ScheduledTask,
    workspaces: &Workspaces,
    target_validator: &TargetValidator,
) -> Result<(), ApplicationError>
where
    Workspaces: WorkspaceRepository,
    TargetValidator: ScheduledTaskTargetValidator,
{
    if !task.enabled {
        return Ok(());
    }
    let workspace = workspaces
        .find_main_workspace(&task.project_id)
        .map_err(ApplicationError::from_workflow_run_repository_error)?
        .filter(ora_domain::Workspace::is_admissible)
        .ok_or(ApplicationError::ScheduledTaskRequiresGitProject)?;
    let WorkspaceLocation::LocalFilesystem { path } = workspace.location else {
        return Err(ApplicationError::ScheduledTaskRequiresGitProject);
    };
    target_validator
        .validate_git_worktree_target(std::path::Path::new(&path))
        .map_err(|error| match error {
            TaskWorktreeProvisionerError::NotARepository => {
                ApplicationError::ScheduledTaskRequiresGitProject
            }
            other => ApplicationError::from_task_worktree_provisioner_error(other),
        })
}
