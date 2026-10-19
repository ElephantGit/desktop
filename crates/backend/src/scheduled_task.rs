use std::sync::Arc;

#[path = "scheduled_task_runtime.rs"]
mod runtime;

use ora_application::{
    CreateScheduledTaskHandler, DeleteScheduledTaskHandler, GetScheduledTaskHandler,
    GitScheduledTaskTargetValidator, ListScheduledTaskHistoryHandler, ListScheduledTasksHandler,
    RevokeScheduledTaskApprovalHandler, UpdateScheduledTaskHandler, UuidScheduledTaskIdGenerator,
};
use ora_contracts::{
    AgentModel, CreateScheduledTaskRequest, CreateScheduledTaskResponse,
    DeleteScheduledTaskRequest, DeleteScheduledTaskResponse, EmptyErrorParams,
    GetScheduledTaskRequest, GetScheduledTaskResponse, ListAgentModelsRequest,
    ListScheduledTaskHistoryRequest, ListScheduledTaskHistoryResponse, ListScheduledTasksRequest,
    ListScheduledTasksResponse, PublicError, RevokeScheduledTaskApprovalRequest,
    RevokeScheduledTaskApprovalResponse, UpdateScheduledTaskRequest, UpdateScheduledTaskResponse,
};
use ora_db::{
    RepositoryPool, SqliteProjectRepository, SqliteScheduledRunRepository,
    SqliteScheduledTaskRepository, SqliteWorkspaceRepository,
};
use ora_domain::ScheduledTaskExecutionLocation;
use ora_scheduler::Scheduler;

use crate::{
    AgentRuntime, BackendError, ErrorClassification, Sessions, TaskApi, clock::SystemClock,
};

use runtime::ScheduledTaskRuntime;

/// Groups concrete Scheduled Task definition handlers shared by runtime adapters.
pub struct ScheduledTaskApi {
    agent_runtime: AgentRuntime,
    workspaces: SqliteWorkspaceRepository,
    create: CreateScheduledTaskHandler<
        SqliteScheduledTaskRepository,
        SqliteProjectRepository,
        SqliteWorkspaceRepository,
        GitScheduledTaskTargetValidator,
        UuidScheduledTaskIdGenerator,
        SystemClock,
    >,
    get: GetScheduledTaskHandler<SqliteScheduledTaskRepository>,
    list: ListScheduledTasksHandler<SqliteScheduledTaskRepository>,
    list_history: ListScheduledTaskHistoryHandler<SqliteScheduledRunRepository>,
    update: UpdateScheduledTaskHandler<
        SqliteScheduledTaskRepository,
        SqliteProjectRepository,
        SqliteWorkspaceRepository,
        GitScheduledTaskTargetValidator,
        SystemClock,
    >,
    delete: DeleteScheduledTaskHandler<SqliteScheduledTaskRepository, SystemClock>,
    revoke_approval: RevokeScheduledTaskApprovalHandler<SqliteScheduledTaskRepository, SystemClock>,
    runtime: Arc<ScheduledTaskRuntime>,
}

impl ScheduledTaskApi {
    /// Builds definition handlers from the shared repository pool.
    pub(crate) fn new(
        pool: RepositoryPool,
        agent_runtime: AgentRuntime,
        scheduler: Scheduler,
        tasks_api: Arc<TaskApi>,
        sessions: Arc<Sessions>,
        clock: SystemClock,
    ) -> Self {
        let repository = Arc::new(SqliteScheduledTaskRepository::new(pool.clone()));
        let projects = Arc::new(SqliteProjectRepository::new(pool.clone()));
        let workspaces = Arc::new(SqliteWorkspaceRepository::new(pool.clone()));
        let run_repository = Arc::new(SqliteScheduledRunRepository::new(pool.clone()));
        let runtime = ScheduledTaskRuntime::new(pool, scheduler, tasks_api, sessions, clock);
        Self {
            agent_runtime,
            workspaces: workspaces.as_ref().clone(),
            create: CreateScheduledTaskHandler::new(
                repository.clone(),
                projects.clone(),
                workspaces.clone(),
                GitScheduledTaskTargetValidator,
                UuidScheduledTaskIdGenerator::new(),
                clock,
            ),
            get: GetScheduledTaskHandler::new(repository.clone()),
            list: ListScheduledTasksHandler::new(repository.clone()),
            list_history: ListScheduledTaskHistoryHandler::new(run_repository),
            update: UpdateScheduledTaskHandler::new(
                repository.clone(),
                projects,
                workspaces,
                GitScheduledTaskTargetValidator,
                clock,
            ),
            delete: DeleteScheduledTaskHandler::new(repository.clone(), clock),
            revoke_approval: RevokeScheduledTaskApprovalHandler::new(repository, clock),
            runtime,
        }
    }

    /// Creates a persisted Scheduled Task definition.
    pub async fn create(
        &self,
        request: CreateScheduledTaskRequest,
    ) -> Result<CreateScheduledTaskResponse, BackendError> {
        let task = self.create.prepare(request)?;
        if task.enabled && task.execution_location == ScheduledTaskExecutionLocation::Local {
            self.validate_agent_model(
                task.project_id.as_ref(),
                task.agent_ref.as_str(),
                task.model.as_deref(),
            )
            .await?;
        }
        let task_for_runtime = task.clone();
        let response = self.create.persist(task).map_err(BackendError::from)?;
        self.runtime.reconcile(task_for_runtime);
        Ok(response)
    }

    /// Loads one Scheduled Task definition.
    pub fn get(
        &self,
        request: GetScheduledTaskRequest,
    ) -> Result<GetScheduledTaskResponse, BackendError> {
        self.get.handle(request).map_err(BackendError::from)
    }

    /// Lists non-deleted Scheduled Task definitions.
    pub fn list(
        &self,
        request: ListScheduledTasksRequest,
    ) -> Result<ListScheduledTasksResponse, BackendError> {
        self.list.handle(request).map_err(BackendError::from)
    }

    /// Lists persisted run and missed facts for one Scheduled Task.
    pub fn list_history(
        &self,
        request: ListScheduledTaskHistoryRequest,
    ) -> Result<ListScheduledTaskHistoryResponse, BackendError> {
        self.list_history
            .handle(request)
            .map_err(BackendError::from)
    }

    /// Updates mutable Scheduled Task definition fields.
    pub async fn update(
        &self,
        request: UpdateScheduledTaskRequest,
    ) -> Result<UpdateScheduledTaskResponse, BackendError> {
        let prepared = self.update.prepare(request)?;
        let task = prepared.task();
        if task.enabled && task.execution_location == ScheduledTaskExecutionLocation::Local {
            self.validate_agent_model(
                task.project_id.as_ref(),
                task.agent_ref.as_str(),
                task.model.as_deref(),
            )
            .await?;
        }
        let task_for_runtime = prepared.task().clone();
        let response = self.update.persist(prepared).map_err(BackendError::from)?;
        self.runtime.reconcile(task_for_runtime);
        Ok(response)
    }

    async fn validate_agent_model(
        &self,
        project_id: &str,
        agent_ref: &str,
        model: Option<&str>,
    ) -> Result<(), BackendError> {
        let workspace = self
            .workspaces
            .find_main_workspace(&ora_domain::ProjectId::new(project_id))
            .map_err(|source| {
                BackendError::internal("scheduled task workspace lookup failed", source)
            })?
            .ok_or_else(|| {
                invalid_runtime_selection("scheduled task project has no main workspace")
            })?;
        let response = self
            .agent_runtime
            .models(ListAgentModelsRequest {
                agent_ref: agent_ref.to_string(),
                workspace_id: workspace.id.to_string(),
            })
            .await?;
        require_requested_model(model, &response.models)
    }

    /// Soft-deletes a Scheduled Task definition.
    pub fn delete(
        &self,
        request: DeleteScheduledTaskRequest,
    ) -> Result<DeleteScheduledTaskResponse, BackendError> {
        let response = self
            .delete
            .handle(request.clone())
            .map_err(BackendError::from)?;
        self.runtime
            .cancel(&ora_domain::ScheduledTaskId::new(request.task_id));
        Ok(response)
    }

    /// Revokes local-background approval and disables the task.
    pub fn revoke_approval(
        &self,
        request: RevokeScheduledTaskApprovalRequest,
    ) -> Result<RevokeScheduledTaskApprovalResponse, BackendError> {
        let task_id = request.task_id.clone();
        let response = self
            .revoke_approval
            .handle(request)
            .map_err(BackendError::from)?;
        self.runtime
            .cancel(&ora_domain::ScheduledTaskId::new(task_id));
        Ok(response)
    }
}

fn require_requested_model(model: Option<&str>, models: &[AgentModel]) -> Result<(), BackendError> {
    let Some(model) = model else {
        return Ok(());
    };
    if models.iter().any(|candidate| candidate.id == model) {
        return Ok(());
    }
    Err(invalid_runtime_selection(
        "scheduled task model is unavailable for the selected agent",
    ))
}

fn invalid_runtime_selection(context: &'static str) -> BackendError {
    BackendError::new(
        ErrorClassification::InvalidRequest,
        PublicError::InvalidRequest(EmptyErrorParams {}),
        context,
    )
}

#[cfg(test)]
mod tests {
    use super::require_requested_model;
    use ora_contracts::AgentModel;

    fn model(id: &str) -> AgentModel {
        AgentModel {
            id: id.to_string(),
            display_name: id.to_string(),
            default: false,
        }
    }

    #[test]
    fn accepts_the_runtime_default_or_an_advertised_model() {
        let models = vec![model("fast"), model("smart")];

        assert!(require_requested_model(None, &models).is_ok());
        assert!(require_requested_model(Some("smart"), &models).is_ok());
    }

    #[test]
    fn rejects_a_model_the_selected_agent_does_not_advertise() {
        let error = require_requested_model(Some("retired"), &[model("smart")])
            .expect_err("reject missing model");

        assert_eq!(
            error.classification(),
            crate::ErrorClassification::InvalidRequest
        );
    }
}
