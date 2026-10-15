use ora_domain::{
    ScheduledMissReason, ScheduledMissedOccurrence, ScheduledOccurrence, ScheduledTask,
};

use super::{
    ScheduledOccurrenceCandidate, ScheduledOccurrenceRepository, ScheduledOccurrenceResolution,
};
use crate::ApplicationError;

/// Reports whether startup recovery found an offline scheduled time worth recording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OfflineRecoveryResolution {
    NothingMissed,
    Recorded(Box<ScheduledOccurrenceResolution>),
}

/// Collapses an offline interval to at most its latest due time for one Scheduled Task.
pub struct RecordOfflineMissedOccurrenceHandler<Repository> {
    repository: Repository,
}

impl<Repository> RecordOfflineMissedOccurrenceHandler<Repository> {
    pub fn new(repository: Repository) -> Self {
        Self { repository }
    }
}

impl<Repository> RecordOfflineMissedOccurrenceHandler<Repository>
where
    Repository: ScheduledOccurrenceRepository,
{
    /// Persists only the latest due point so a long shutdown never creates an unbounded history.
    pub fn handle(
        &self,
        task: &ScheduledTask,
        scheduled_times: impl IntoIterator<Item = i64>,
        observed_at: i64,
    ) -> Result<OfflineRecoveryResolution, ApplicationError> {
        let Some(scheduled_at) = scheduled_times
            .into_iter()
            .filter(|scheduled_at| *scheduled_at <= observed_at)
            .max()
        else {
            return Ok(OfflineRecoveryResolution::NothingMissed);
        };
        let missed = ScheduledMissedOccurrence {
            occurrence: ScheduledOccurrence {
                task_id: task.id.clone(),
                scheduled_at,
            },
            reason: ScheduledMissReason::AppNotRunning,
            observed_at,
        };
        self.repository
            .resolve_scheduled_occurrence(ScheduledOccurrenceCandidate::Missed(missed))
            .map(Box::new)
            .map(OfflineRecoveryResolution::Recorded)
            .map_err(ApplicationError::from_scheduled_task_repository_error)
    }
}
