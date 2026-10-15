use ora_domain::{ScheduledTask, ScheduledTaskId};

use crate::RepositoryError;

/// Defines persistence operations required by scheduled-task definition use cases.
pub trait ScheduledTaskRepository {
    /// Persists a newly created task definition.
    fn create_scheduled_task(&self, task: ScheduledTask) -> Result<ScheduledTask, RepositoryError>;

    /// Loads one visible task definition by identifier.
    fn find_scheduled_task(
        &self,
        task_id: &ScheduledTaskId,
    ) -> Result<Option<ScheduledTask>, RepositoryError>;

    /// Lists visible task definitions in deterministic storage order.
    fn list_scheduled_tasks(&self) -> Result<Vec<ScheduledTask>, RepositoryError>;

    /// Replaces one visible task definition.
    fn update_scheduled_task(
        &self,
        task: ScheduledTask,
    ) -> Result<Option<ScheduledTask>, RepositoryError>;

    /// Marks one visible task definition deleted without removing its history.
    fn soft_delete_scheduled_task(
        &self,
        task_id: &ScheduledTaskId,
        deleted_at: i64,
    ) -> Result<bool, RepositoryError>;
}

/// Supplies stable identifiers for newly created scheduled-task definitions.
pub trait ScheduledTaskIdGenerator {
    /// Produces one new scheduled-task identifier.
    fn generate_scheduled_task_id(&self) -> ScheduledTaskId;
}
