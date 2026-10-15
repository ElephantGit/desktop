use ora_domain::{
    ScheduledMissedOccurrence, ScheduledOccurrence, ScheduledRun, ScheduledRunId, ScheduledTask,
    ScheduledTaskId,
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

/// Supplies the fact a scheduler wants to persist for one planned occurrence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduledOccurrenceCandidate {
    Run(ScheduledRun),
    Missed(ScheduledMissedOccurrence),
}

impl ScheduledOccurrenceCandidate {
    /// Returns the stable planned identity shared by run and missed facts.
    pub fn occurrence(&self) -> &ScheduledOccurrence {
        match self {
            Self::Run(run) => &run.occurrence,
            Self::Missed(missed) => &missed.occurrence,
        }
    }
}

/// Distinguishes newly persisted occurrence facts from idempotent replays.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduledOccurrenceResolution {
    NotDue,
    Admitted(ScheduledRun),
    ExistingRun(ScheduledRun),
    Missed(ScheduledMissedOccurrence),
    ExistingMissed(ScheduledMissedOccurrence),
}

/// Defines the atomic persistence boundary shared by admitted and missed occurrences.
pub trait ScheduledOccurrenceRepository {
    /// Inserts one outcome at most once and returns the existing outcome on replay.
    fn resolve_scheduled_occurrence(
        &self,
        candidate: ScheduledOccurrenceCandidate,
    ) -> Result<ScheduledOccurrenceResolution, RepositoryError>;
}

/// Supplies stable identifiers for newly admitted Scheduled Runs.
pub trait ScheduledRunIdGenerator {
    /// Produces one new Scheduled Run identifier.
    fn generate_scheduled_run_id(&self) -> ScheduledRunId;
}
