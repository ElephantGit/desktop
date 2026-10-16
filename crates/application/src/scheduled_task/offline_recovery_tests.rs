use std::sync::Mutex;

use ora_domain::{
    AgentRef, AuditFields, ProjectId, ScheduledMissReason, ScheduledTask, ScheduledTaskApproval,
    ScheduledTaskExecutionLocation, ScheduledTaskId, ScheduledTaskSchedule,
};
use pretty_assertions::assert_eq;

use super::{
    OfflineRecoveryResolution, RecordOfflineMissedOccurrenceHandler, ScheduledOccurrenceCandidate,
    ScheduledOccurrenceRepository, ScheduledOccurrenceResolution,
};
use crate::RepositoryError;

#[test]
fn offline_interval_records_only_its_latest_expired_point() {
    let repository = RecordingOccurrenceRepository::default();
    let handler = RecordOfflineMissedOccurrenceHandler::new(&repository);

    // 100_000 and 200_000 are past their ten-minute window at 800_001; 300_000 and 700_000 are
    // still inside theirs, so only the latest expired point may be recorded.
    let result = handler
        .handle(
            &scheduled_task(),
            [100_000, 200_000, 300_000, 700_000],
            800_001,
        )
        .expect("record offline interval");

    let OfflineRecoveryResolution::Recorded(resolution) = result else {
        panic!("offline recovery must record one missed fact");
    };
    let ScheduledOccurrenceResolution::Missed(missed) = *resolution else {
        panic!("offline recovery must persist a missed outcome");
    };
    assert_eq!(missed.occurrence.scheduled_at, 200_000);
    assert_eq!(missed.reason, ScheduledMissReason::AppNotRunning);
    assert_eq!(missed.observed_at, 800_001);
    assert_eq!(repository.calls(), 1);
}

#[test]
fn a_point_inside_its_lateness_window_is_left_for_live_admission() {
    let repository = RecordingOccurrenceRepository::default();
    let handler = RecordOfflineMissedOccurrenceHandler::new(&repository);

    // Restarting three minutes after the due time is a normal late trigger per the ADR, not a
    // missed occurrence; recording it would permanently block the live run.
    assert_eq!(
        handler
            .handle(&scheduled_task(), [1_000_000], 1_180_000)
            .expect("ignore inside-window point"),
        OfflineRecoveryResolution::NothingMissed
    );
    assert_eq!(repository.calls(), 0);
}

#[test]
fn the_exact_window_boundary_still_belongs_to_live_admission() {
    let repository = RecordingOccurrenceRepository::default();
    let handler = RecordOfflineMissedOccurrenceHandler::new(&repository);

    assert_eq!(
        handler
            .handle(&scheduled_task(), [1_000_000], 1_600_000)
            .expect("ignore boundary point"),
        OfflineRecoveryResolution::NothingMissed
    );
    assert_eq!(repository.calls(), 0);
}

#[test]
fn offline_interval_without_an_expired_point_records_nothing() {
    let repository = RecordingOccurrenceRepository::default();
    let handler = RecordOfflineMissedOccurrenceHandler::new(&repository);

    assert_eq!(
        handler
            .handle(&scheduled_task(), [500_000, 600_000], 400_000)
            .expect("ignore future points"),
        OfflineRecoveryResolution::NothingMissed
    );
    assert_eq!(repository.calls(), 0);
}

#[derive(Default)]
struct RecordingOccurrenceRepository {
    candidates: Mutex<Vec<ScheduledOccurrenceCandidate>>,
}

impl RecordingOccurrenceRepository {
    fn calls(&self) -> usize {
        self.candidates.lock().expect("lock candidates").len()
    }
}

impl ScheduledOccurrenceRepository for &RecordingOccurrenceRepository {
    fn resolve_scheduled_occurrence(
        &self,
        candidate: ScheduledOccurrenceCandidate,
    ) -> Result<ScheduledOccurrenceResolution, RepositoryError> {
        let resolution = match &candidate {
            ScheduledOccurrenceCandidate::Run(run) => {
                ScheduledOccurrenceResolution::Admitted((**run).clone())
            }
            ScheduledOccurrenceCandidate::Missed(missed) => {
                ScheduledOccurrenceResolution::Missed(missed.clone())
            }
        };
        self.candidates
            .lock()
            .expect("lock candidates")
            .push(candidate);
        Ok(resolution)
    }
}

fn scheduled_task() -> ScheduledTask {
    ScheduledTask::new(
        ScheduledTaskId::new("scheduled-task-1"),
        "Quality report",
        true,
        ScheduledTaskSchedule::every_hours(4).expect("hourly schedule"),
        ScheduledTaskExecutionLocation::Local,
        ProjectId::new("project-1"),
        AgentRef::parse("ora/agent").expect("agent ref"),
        Some("default".to_string()),
        "Run the quality gate",
        Some(ScheduledTaskApproval::full_auto_in_worktree(10)),
        AuditFields::new(10, 10, false),
    )
    .expect("task")
}
