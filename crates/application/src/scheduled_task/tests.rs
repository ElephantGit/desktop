use std::sync::{Arc, Mutex};

use ora_contracts::{
    CreateScheduledTaskRequest, RevokeScheduledTaskApprovalRequest, ScheduledTaskApprovalPolicy,
    ScheduledTaskExecutionLocation, ScheduledTaskSchedule, UpdateScheduledTaskRequest,
};
use ora_domain::{
    AuditFields, Project, ProjectId, ScheduledTask, ScheduledTaskId, WorkspaceLocation,
};
use pretty_assertions::assert_eq;

use super::{
    CreateScheduledTaskHandler, RevokeScheduledTaskApprovalHandler, ScheduledTaskIdGenerator,
    ScheduledTaskRepository, UpdateScheduledTaskHandler,
};
use crate::{ApplicationError, Clock, ProjectRepository, RepositoryError};

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
fn prompt_and_schedule_changes_increment_revision_without_reconsent() {
    let repository = Arc::new(MemoryScheduledTaskRepository::default());
    let created = create_handler(repository.clone())
        .handle(create_request())
        .expect("create task")
        .task;
    let consented_at = created.approval.expect("approval").consented_at;
    let handler = UpdateScheduledTaskHandler::new(repository, projects(), FixedClock(NOW + 1_000));

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

fn create_handler(
    repository: Arc<MemoryScheduledTaskRepository>,
) -> CreateScheduledTaskHandler<
    MemoryScheduledTaskRepository,
    MemoryProjectRepository,
    FixedIdGenerator,
    FixedClock,
> {
    CreateScheduledTaskHandler::new(repository, projects(), FixedIdGenerator, FixedClock(NOW))
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
