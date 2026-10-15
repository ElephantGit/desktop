use ora_domain::{
    ScheduledMissedOccurrence, ScheduledOccurrence, ScheduledOccurrenceTiming, ScheduledRun,
    ScheduledTask,
};

use super::{
    ScheduledOccurrenceCandidate, ScheduledOccurrenceRepository, ScheduledOccurrenceResolution,
    ScheduledRunIdGenerator,
};
use crate::ApplicationError;

/// Admits a planned occurrence while freezing the task inputs used by the future execution.
pub struct AdmitScheduledOccurrenceHandler<Repository, IdGenerator> {
    repository: Repository,
    id_generator: IdGenerator,
}

impl<Repository, IdGenerator> AdmitScheduledOccurrenceHandler<Repository, IdGenerator> {
    pub fn new(repository: Repository, id_generator: IdGenerator) -> Self {
        Self {
            repository,
            id_generator,
        }
    }
}

impl<Repository, IdGenerator> AdmitScheduledOccurrenceHandler<Repository, IdGenerator>
where
    Repository: ScheduledOccurrenceRepository,
    IdGenerator: ScheduledRunIdGenerator,
{
    /// Resolves one live observation at the fixed ten-minute boundary and persists it once.
    pub fn handle(
        &self,
        task: &ScheduledTask,
        scheduled_at: i64,
        accepted_at: i64,
    ) -> Result<ScheduledOccurrenceResolution, ApplicationError> {
        let occurrence = ScheduledOccurrence {
            task_id: task.id.clone(),
            scheduled_at,
        };
        let candidate = match occurrence.timing_at(accepted_at) {
            ScheduledOccurrenceTiming::NotDue => {
                return Ok(ScheduledOccurrenceResolution::NotDue);
            }
            ScheduledOccurrenceTiming::Due => {
                ScheduledOccurrenceCandidate::Run(ScheduledRun::accepted(
                    self.id_generator.generate_scheduled_run_id(),
                    occurrence,
                    task,
                    accepted_at,
                ))
            }
            ScheduledOccurrenceTiming::Missed(reason) => {
                ScheduledOccurrenceCandidate::Missed(ScheduledMissedOccurrence {
                    occurrence,
                    reason,
                    observed_at: accepted_at,
                })
            }
        };
        self.repository
            .resolve_scheduled_occurrence(candidate)
            .map_err(ApplicationError::from_scheduled_task_repository_error)
    }
}
