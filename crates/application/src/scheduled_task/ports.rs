use ora_domain::{
    ScheduledOccurrence, ScheduledRun, ScheduledRunId, ScheduledTask, ScheduledTaskId,
};

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

/// Distinguishes a newly persisted run from an idempotent replay of the same occurrence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduledRunAdmission {
    Admitted(ScheduledRun),
    Existing(ScheduledRun),
}

impl ScheduledRunAdmission {
    /// Returns the single run fact associated with the occurrence.
    pub fn run(&self) -> &ScheduledRun {
        match self {
            Self::Admitted(run) | Self::Existing(run) => run,
        }
    }
}

/// Defines the atomic persistence boundary for planned occurrence admission.
pub trait ScheduledRunRepository {
    /// Inserts the candidate at most once and returns the existing fact on replay.
    fn admit_scheduled_run(
        &self,
        candidate: ScheduledRun,
    ) -> Result<ScheduledRunAdmission, RepositoryError>;

    /// Loads the run already associated with a planned occurrence.
    fn find_scheduled_run(
        &self,
        occurrence: &ScheduledOccurrence,
    ) -> Result<Option<ScheduledRun>, RepositoryError>;
}

/// Supplies stable identifiers for newly admitted Scheduled Runs.
pub trait ScheduledRunIdGenerator {
    /// Produces one new Scheduled Run identifier.
    fn generate_scheduled_run_id(&self) -> ScheduledRunId;
}
