use ora_domain::{
    ScheduledMissedOccurrence, ScheduledOccurrence, ScheduledRun, ScheduledRunId,
    ScheduledRunTransition, ScheduledTask, ScheduledTaskHistoryEntry, ScheduledTaskId,
};
use std::path::Path;

use crate::{RepositoryError, TaskWorktreeProvisionerError};

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
        expected_updated_at: i64,
    ) -> Result<ScheduledTaskUpdateResolution, RepositoryError>;

    /// Marks one visible task definition deleted without removing its history.
    fn soft_delete_scheduled_task(
        &self,
        task_id: &ScheduledTaskId,
        deleted_at: i64,
    ) -> Result<bool, RepositoryError>;
}

/// Distinguishes a committed replacement from a missing or concurrently changed definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduledTaskUpdateResolution {
    Updated(Box<ScheduledTask>),
    NotFound,
    Conflict,
}

/// Supplies stable identifiers for newly created scheduled-task definitions.
pub trait ScheduledTaskIdGenerator {
    /// Produces one new scheduled-task identifier.
    fn generate_scheduled_task_id(&self) -> ScheduledTaskId;
}

/// Verifies that a local Scheduled Task target can use the isolated-worktree execution mode.
pub trait ScheduledTaskTargetValidator {
    /// Checks the configured project directory without creating a branch or worktree.
    fn validate_git_worktree_target(
        &self,
        project_root: &Path,
    ) -> Result<(), TaskWorktreeProvisionerError>;
}

/// Supplies the fact a scheduler wants to persist for one planned occurrence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduledOccurrenceCandidate {
    Run(Box<ScheduledRun>),
    Missed(ScheduledMissedOccurrence),
}

impl ScheduledOccurrenceCandidate {
    /// Stores a scheduled run behind one pointer so the short missed fact stays compact.
    pub fn run(run: ScheduledRun) -> Self {
        Self::Run(Box::new(run))
    }

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

/// Defines newest-first history reads independently from occurrence admission writes.
pub trait ScheduledTaskHistoryRepository {
    fn list_scheduled_task_history(
        &self,
        task_id: &ScheduledTaskId,
    ) -> Result<Vec<ScheduledTaskHistoryEntry>, RepositoryError>;
}

/// Distinguishes a committed lifecycle write from missing or stale execution callbacks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduledRunTransitionResolution {
    Updated(ScheduledRun),
    NotFound,
    InvalidState(ScheduledRun),
}

/// Persists forward-only Scheduled Run lifecycle changes atomically.
pub trait ScheduledRunLifecycleRepository {
    /// Marks active runs from an earlier Desktop process as failed during startup recovery.
    fn fail_interrupted_scheduled_runs(
        &self,
        interrupted_at: i64,
    ) -> Result<usize, RepositoryError>;

    fn transition_scheduled_run(
        &self,
        run_id: &ScheduledRunId,
        transition: ScheduledRunTransition,
        transitioned_at: i64,
    ) -> Result<ScheduledRunTransitionResolution, RepositoryError>;
}

/// Supplies stable identifiers for newly admitted Scheduled Runs.
pub trait ScheduledRunIdGenerator {
    /// Produces one new Scheduled Run identifier.
    fn generate_scheduled_run_id(&self) -> ScheduledRunId;
}
