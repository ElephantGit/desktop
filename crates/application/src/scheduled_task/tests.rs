use std::sync::{Arc, Mutex};

use ora_contracts::{
    CreateScheduledTaskRequest, RevokeScheduledTaskApprovalRequest, ScheduledTaskApprovalPolicy,
    ScheduledTaskExecutionLocation, ScheduledTaskSchedule, UpdateScheduledTaskRequest,
};
use ora_domain::{
    AuditFields, Project, ProjectId, ScheduledTask, ScheduledTaskId, Workspace, WorkspaceId,
    WorkspaceKind, WorkspaceLifecycle, WorkspaceLocation,
};
use pretty_assertions::assert_eq;

use super::{
    CreateScheduledTaskHandler, RevokeScheduledTaskApprovalHandler, ScheduledTaskIdGenerator,
    ScheduledTaskRepository, ScheduledTaskTargetValidator, UpdateScheduledTaskHandler,
};
use crate::{
    ApplicationError, Clock, ProjectRepository, RepositoryError, TaskWorktreeProvisionerError,
    WorkspaceRepository,
};

const NOW: i64 = 1_760_000_000_000;

#[test]
fn cloud_definitions_are_rejected_while_cloud_is_unavailable() {
    let repository = Arc::new(MemoryScheduledTaskRepository::default());
    let handler = create_handler(repository.clone());
    let mut request = create_request();
    request.execution_location = ScheduledTaskExecutionLocation::Cloud;

    let result = handler.handle(request);

    assert_eq!(result, Err(ApplicationError::ScheduledTaskCloudUnavailable));
    assert!(repository.tasks.lock().expect("lock tasks").is_empty());
}

#[test]
fn enabled_local_task_requires_explicit_unattended_approval() {
    let repository = Arc::new(MemoryScheduledTaskRepository::default());
    let handler = create_handler(repository.clone());
    let mut request = create_request();
    request.approval_policy = None;

    let result = handler.handle(request);

    assert_eq!(result, Err(ApplicationError::ScheduledTaskApprovalRequired));
    assert!(repository.tasks.lock().expect("lock tasks").is_empty());
}

#[test]
fn approved_local_task_is_created_at_revision_one() {
    let repository = Arc::new(MemoryScheduledTaskRepository::default());
    let handler = create_handler(repository);

    let response = handler.handle(create_request()).expect("create task");

    assert!(response.task.enabled);
    assert_eq!(response.task.revision, 1);
    assert_eq!(response.task.approval.expect("approval").consented_at, NOW);
}

#[test]
fn prepared_definition_is_not_written_until_runtime_eligibility_can_be_checked() {
    let repository = Arc::new(MemoryScheduledTaskRepository::default());
    let handler = create_handler(repository.clone());

    let prepared = handler.prepare(create_request()).expect("prepare task");
    assert!(repository.tasks.lock().expect("lock tasks").is_empty());

    handler.persist(prepared).expect("persist prepared task");
    assert_eq!(repository.tasks.lock().expect("lock tasks").len(), 1);
}

#[test]
fn prompt_and_schedule_changes_increment_revision_without_reconsent() {
    let repository = Arc::new(MemoryScheduledTaskRepository::default());
    let created = create_handler(repository.clone())
        .handle(create_request())
        .expect("create task")
        .task;
    let consented_at = created.approval.expect("approval").consented_at;
    let handler = UpdateScheduledTaskHandler::new(
        repository,
        projects(),
        workspaces(),
        AcceptingTargetValidator,
        FixedClock(NOW + 1_000),
    );

    let response = handler
        .handle(UpdateScheduledTaskRequest {
            task_id: created.id,
            name: created.name,
            enabled: true,
            schedule: ScheduledTaskSchedule::Daily {
                hour: 9,
                minute: 30,
            },
            project_id: created.project_id,
            agent_ref: created.agent_ref,
            model: created.model,
            prompt: "Run the full quality gate".to_string(),
            approval_policy: None,
        })
        .expect("update task");

    assert_eq!(response.task.revision, 2);
    assert_eq!(
        response.task.approval.expect("approval").consented_at,
        consented_at
    );
}

#[test]
fn changing_project_requires_fresh_consent_before_remaining_enabled() {
    let repository = Arc::new(MemoryScheduledTaskRepository::default());
    let created = create_handler(repository.clone())
        .handle(create_request())
        .expect("create task")
        .task;
    let handler = UpdateScheduledTaskHandler::new(
        repository,
        Arc::new(MemoryProjectRepository::with_projects(vec![
            git_project("project-1"),
            git_project("project-2"),
        ])),
        Arc::new(MemoryWorkspaceRepository::with_workspaces(vec![
            main_workspace("project-1"),
            main_workspace("project-2"),
        ])),
        AcceptingTargetValidator,
        FixedClock(NOW + 1_000),
    );

    let result = handler.handle(UpdateScheduledTaskRequest {
        task_id: created.id,
        name: created.name,
        enabled: true,
        schedule: created.schedule,
        project_id: "project-2".to_string(),
        agent_ref: created.agent_ref,
        model: created.model,
        prompt: created.prompt,
        approval_policy: None,
    });

    assert_eq!(result, Err(ApplicationError::ScheduledTaskApprovalRequired));
}

#[test]
fn revoking_approval_disables_the_task_without_changing_revision() {
    let repository = Arc::new(MemoryScheduledTaskRepository::default());
    let created = create_handler(repository.clone())
        .handle(create_request())
        .expect("create task")
        .task;
    let handler = RevokeScheduledTaskApprovalHandler::new(repository, FixedClock(NOW + 1_000));

    let response = handler
        .handle(RevokeScheduledTaskApprovalRequest {
            task_id: created.id,
        })
        .expect("revoke approval");

    assert!(!response.task.enabled);
    assert_eq!(response.task.approval, None);
    assert_eq!(response.task.revision, 1);
}

#[test]
fn enabled_local_task_rejects_a_target_without_git_worktree_capability() {
    let repository = Arc::new(MemoryScheduledTaskRepository::default());
    let handler = CreateScheduledTaskHandler::new(
        repository.clone(),
        projects(),
        workspaces(),
        RejectingTargetValidator,
        FixedIdGenerator,
        FixedClock(NOW),
    );

    let result = handler.handle(create_request());

    assert_eq!(
        result,
        Err(ApplicationError::ScheduledTaskRequiresGitProject)
    );
    assert!(repository.tasks.lock().expect("lock tasks").is_empty());
}

#[test]
fn disabled_local_draft_does_not_require_a_current_git_worktree_target() {
    let repository = Arc::new(MemoryScheduledTaskRepository::default());
    let handler = CreateScheduledTaskHandler::new(
        repository,
        projects(),
        Arc::new(MemoryWorkspaceRepository::with_workspaces(Vec::new())),
        RejectingTargetValidator,
        FixedIdGenerator,
        FixedClock(NOW),
    );
    let mut request = create_request();
    request.enabled = false;

    let response = handler.handle(request).expect("create disabled draft");

    assert!(!response.task.enabled);
}

fn create_handler(
    repository: Arc<MemoryScheduledTaskRepository>,
) -> CreateScheduledTaskHandler<
    MemoryScheduledTaskRepository,
    MemoryProjectRepository,
    MemoryWorkspaceRepository,
    AcceptingTargetValidator,
    FixedIdGenerator,
    FixedClock,
> {
    CreateScheduledTaskHandler::new(
        repository,
        projects(),
        workspaces(),
        AcceptingTargetValidator,
        FixedIdGenerator,
        FixedClock(NOW),
    )
}

fn create_request() -> CreateScheduledTaskRequest {
    CreateScheduledTaskRequest {
        name: "Daily quality report".to_string(),
        enabled: true,
        schedule: ScheduledTaskSchedule::EveryHours { interval: 4 },
        execution_location: ScheduledTaskExecutionLocation::Local,
        project_id: "project-1".to_string(),
        agent_ref: "ora/claude-code".to_string(),
        model: Some("default".to_string()),
        prompt: "Run the quality gate".to_string(),
        approval_policy: Some(ScheduledTaskApprovalPolicy::FullAutoInWorktree),
    }
}

fn projects() -> Arc<MemoryProjectRepository> {
    Arc::new(MemoryProjectRepository::with_projects(vec![git_project(
        "project-1",
    )]))
}

fn workspaces() -> Arc<MemoryWorkspaceRepository> {
    Arc::new(MemoryWorkspaceRepository::with_workspaces(vec![
        main_workspace("project-1"),
    ]))
}

fn git_project(id: &str) -> Project {
    Project {
        id: ProjectId::new(id),
        name: id.to_string(),
        repository_kind: "git".to_string(),
        repository_url: None,
        default_branch: Some("main".to_string()),
        audit_fields: AuditFields::new(NOW, NOW, false),
    }
}

fn main_workspace(project_id: &str) -> Workspace {
    Workspace::new(
        WorkspaceId::new(format!("workspace-{project_id}")),
        ProjectId::new(project_id),
        WorkspaceKind::Main,
        WorkspaceLocation::local_filesystem(format!("/workspace/{project_id}")),
        WorkspaceLifecycle::Active,
        AuditFields::new(NOW, NOW, false),
    )
}

#[derive(Default)]
struct MemoryScheduledTaskRepository {
    tasks: Mutex<Vec<ScheduledTask>>,
}

impl ScheduledTaskRepository for MemoryScheduledTaskRepository {
    fn create_scheduled_task(&self, task: ScheduledTask) -> Result<ScheduledTask, RepositoryError> {
        self.tasks.lock().expect("lock tasks").push(task.clone());
        Ok(task)
    }

    fn find_scheduled_task(
        &self,
        task_id: &ScheduledTaskId,
    ) -> Result<Option<ScheduledTask>, RepositoryError> {
        Ok(self
            .tasks
            .lock()
            .expect("lock tasks")
            .iter()
            .find(|task| &task.id == task_id && !task.audit_fields.is_deleted)
            .cloned())
    }

    fn list_scheduled_tasks(&self) -> Result<Vec<ScheduledTask>, RepositoryError> {
        Ok(self
            .tasks
            .lock()
            .expect("lock tasks")
            .iter()
            .filter(|task| !task.audit_fields.is_deleted)
            .cloned()
            .collect())
    }

    fn update_scheduled_task(
        &self,
        task: ScheduledTask,
    ) -> Result<Option<ScheduledTask>, RepositoryError> {
        let mut tasks = self.tasks.lock().expect("lock tasks");
        let Some(existing) = tasks
            .iter_mut()
            .find(|existing| existing.id == task.id && !existing.audit_fields.is_deleted)
        else {
            return Ok(None);
        };
        *existing = task.clone();
        Ok(Some(task))
    }

    fn soft_delete_scheduled_task(
        &self,
        task_id: &ScheduledTaskId,
        deleted_at: i64,
    ) -> Result<bool, RepositoryError> {
        let mut tasks = self.tasks.lock().expect("lock tasks");
        let Some(task) = tasks
            .iter_mut()
            .find(|task| &task.id == task_id && !task.audit_fields.is_deleted)
        else {
            return Ok(false);
        };
        task.audit_fields.is_deleted = true;
        task.audit_fields.updated_at = deleted_at;
        Ok(true)
    }
}

struct MemoryProjectRepository {
    projects: Mutex<Vec<Project>>,
}

impl MemoryProjectRepository {
    fn with_projects(projects: Vec<Project>) -> Self {
        Self {
            projects: Mutex::new(projects),
        }
    }
}

impl ProjectRepository for MemoryProjectRepository {
    fn create_project(
        &self,
        project: Project,
        _main_workspace_location: WorkspaceLocation,
    ) -> Result<Project, RepositoryError> {
        self.projects
            .lock()
            .expect("lock projects")
            .push(project.clone());
        Ok(project)
    }

    fn find_project(&self, project_id: &ProjectId) -> Result<Option<Project>, RepositoryError> {
        Ok(self
            .projects
            .lock()
            .expect("lock projects")
            .iter()
            .find(|project| &project.id == project_id && !project.audit_fields.is_deleted)
            .cloned())
    }

    fn list_projects(&self) -> Result<Vec<Project>, RepositoryError> {
        Ok(self.projects.lock().expect("lock projects").clone())
    }

    fn update_project(&self, project: Project) -> Result<Project, RepositoryError> {
        let mut projects = self.projects.lock().expect("lock projects");
        let existing = projects
            .iter_mut()
            .find(|existing| existing.id == project.id)
            .expect("project exists");
        *existing = project.clone();
        Ok(project)
    }

    fn soft_delete_project(
        &self,
        project_id: &ProjectId,
        deleted_at: i64,
    ) -> Result<bool, RepositoryError> {
        let mut projects = self.projects.lock().expect("lock projects");
        let Some(project) = projects
            .iter_mut()
            .find(|project| &project.id == project_id && !project.audit_fields.is_deleted)
        else {
            return Ok(false);
        };
        project.audit_fields.is_deleted = true;
        project.audit_fields.updated_at = deleted_at;
        Ok(true)
    }
}

struct MemoryWorkspaceRepository {
    workspaces: Vec<Workspace>,
}

impl MemoryWorkspaceRepository {
    fn with_workspaces(workspaces: Vec<Workspace>) -> Self {
        Self { workspaces }
    }
}

impl WorkspaceRepository for MemoryWorkspaceRepository {
    fn find_workspace(
        &self,
        workspace_id: &WorkspaceId,
    ) -> Result<Option<Workspace>, RepositoryError> {
        Ok(self
            .workspaces
            .iter()
            .find(|workspace| &workspace.id == workspace_id)
            .cloned())
    }

    fn find_main_workspace(
        &self,
        project_id: &ProjectId,
    ) -> Result<Option<Workspace>, RepositoryError> {
        Ok(self
            .workspaces
            .iter()
            .find(|workspace| {
                &workspace.project_id == project_id && workspace.kind == WorkspaceKind::Main
            })
            .cloned())
    }

    fn is_provisioning_ready(&self, workspace_id: &WorkspaceId) -> Result<bool, RepositoryError> {
        Ok(self
            .workspaces
            .iter()
            .any(|workspace| &workspace.id == workspace_id && workspace.is_admissible()))
    }
}

#[derive(Clone, Copy)]
struct AcceptingTargetValidator;

impl ScheduledTaskTargetValidator for AcceptingTargetValidator {
    fn validate_git_worktree_target(
        &self,
        _project_root: &std::path::Path,
    ) -> Result<(), TaskWorktreeProvisionerError> {
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct RejectingTargetValidator;

impl ScheduledTaskTargetValidator for RejectingTargetValidator {
    fn validate_git_worktree_target(
        &self,
        _project_root: &std::path::Path,
    ) -> Result<(), TaskWorktreeProvisionerError> {
        Err(TaskWorktreeProvisionerError::NotARepository)
    }
}

#[derive(Clone, Copy)]
struct FixedIdGenerator;

impl ScheduledTaskIdGenerator for FixedIdGenerator {
    fn generate_scheduled_task_id(&self) -> ScheduledTaskId {
        ScheduledTaskId::new("scheduled-task-1")
    }
}

#[derive(Clone, Copy)]
struct FixedClock(i64);

impl Clock for FixedClock {
    fn now_timestamp_millis(&self) -> i64 {
        self.0
    }
}
