use ora_domain::{ScheduledOccurrence, ScheduledRun, ScheduledTask};

use super::{ScheduledRunAdmission, ScheduledRunIdGenerator, ScheduledRunRepository};
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
    Repository: ScheduledRunRepository,
    IdGenerator: ScheduledRunIdGenerator,
{
    /// Returns the first persisted run when the same planned time is delivered repeatedly.
    pub fn handle(
        &self,
        task: &ScheduledTask,
        scheduled_at: i64,
        accepted_at: i64,
    ) -> Result<ScheduledRunAdmission, ApplicationError> {
        let occurrence = ScheduledOccurrence {
            task_id: task.id.clone(),
            scheduled_at,
        };
        let candidate = ScheduledRun::accepted(
            self.id_generator.generate_scheduled_run_id(),
            occurrence,
            task,
            accepted_at,
        );
        self.repository
            .admit_scheduled_run(candidate)
            .map_err(ApplicationError::from_scheduled_task_repository_error)
    }
}
