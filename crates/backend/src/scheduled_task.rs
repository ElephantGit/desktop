use std::sync::Arc;

use ora_application::{
    CreateScheduledTaskHandler, DeleteScheduledTaskHandler, GetScheduledTaskHandler,
    ListScheduledTasksHandler, RevokeScheduledTaskApprovalHandler, UpdateScheduledTaskHandler,
    UuidScheduledTaskIdGenerator,
};
use ora_contracts::{
    CreateScheduledTaskRequest, CreateScheduledTaskResponse, DeleteScheduledTaskRequest,
    DeleteScheduledTaskResponse, GetScheduledTaskRequest, GetScheduledTaskResponse,
    ListScheduledTasksRequest, ListScheduledTasksResponse, RevokeScheduledTaskApprovalRequest,
    RevokeScheduledTaskApprovalResponse, UpdateScheduledTaskRequest, UpdateScheduledTaskResponse,
};
use ora_db::{RepositoryPool, SqliteProjectRepository, SqliteScheduledTaskRepository};

use crate::{BackendError, clock::SystemClock};

/// Groups concrete Scheduled Task definition handlers shared by runtime adapters.
pub struct ScheduledTaskApi {
    create: CreateScheduledTaskHandler<
        SqliteScheduledTaskRepository,
        SqliteProjectRepository,
        UuidScheduledTaskIdGenerator,
        SystemClock,
    >,
    get: GetScheduledTaskHandler<SqliteScheduledTaskRepository>,
    list: ListScheduledTasksHandler<SqliteScheduledTaskRepository>,
    update: UpdateScheduledTaskHandler<
        SqliteScheduledTaskRepository,
        SqliteProjectRepository,
        SystemClock,
    >,
    delete: DeleteScheduledTaskHandler<SqliteScheduledTaskRepository, SystemClock>,
    revoke_approval: RevokeScheduledTaskApprovalHandler<SqliteScheduledTaskRepository, SystemClock>,
}

impl ScheduledTaskApi {
    /// Builds definition handlers from the shared repository pool.
    pub(crate) fn new(pool: RepositoryPool, clock: SystemClock) -> Self {
        let repository = Arc::new(SqliteScheduledTaskRepository::new(pool.clone()));
        let projects = Arc::new(SqliteProjectRepository::new(pool));
        Self {
            create: CreateScheduledTaskHandler::new(
                repository.clone(),
                projects.clone(),
                UuidScheduledTaskIdGenerator::new(),
                clock,
            ),
            get: GetScheduledTaskHandler::new(repository.clone()),
            list: ListScheduledTasksHandler::new(repository.clone()),
            update: UpdateScheduledTaskHandler::new(repository.clone(), projects, clock),
            delete: DeleteScheduledTaskHandler::new(repository.clone(), clock),
            revoke_approval: RevokeScheduledTaskApprovalHandler::new(repository, clock),
        }
    }

    /// Creates a persisted Scheduled Task definition.
    pub fn create(
        &self,
        request: CreateScheduledTaskRequest,
    ) -> Result<CreateScheduledTaskResponse, BackendError> {
        self.create.handle(request).map_err(BackendError::from)
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

    /// Updates mutable Scheduled Task definition fields.
    pub fn update(
        &self,
        request: UpdateScheduledTaskRequest,
    ) -> Result<UpdateScheduledTaskResponse, BackendError> {
        self.update.handle(request).map_err(BackendError::from)
    }

    /// Soft-deletes a Scheduled Task definition.
    pub fn delete(
        &self,
        request: DeleteScheduledTaskRequest,
    ) -> Result<DeleteScheduledTaskResponse, BackendError> {
        self.delete.handle(request).map_err(BackendError::from)
    }

    /// Revokes local-background approval and disables the task.
    pub fn revoke_approval(
        &self,
        request: RevokeScheduledTaskApprovalRequest,
    ) -> Result<RevokeScheduledTaskApprovalResponse, BackendError> {
        self.revoke_approval
            .handle(request)
            .map_err(BackendError::from)
    }
}
