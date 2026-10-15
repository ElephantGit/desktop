use std::sync::Mutex;

use ora_domain::{
    AgentRef, AuditFields, ProjectId, ScheduledMissReason, ScheduledOccurrence, ScheduledRunId,
    ScheduledTask, ScheduledTaskApproval, ScheduledTaskExecutionLocation, ScheduledTaskId,
    ScheduledTaskSchedule,
};
use pretty_assertions::assert_eq;

use super::{
    AdmitScheduledOccurrenceHandler, ScheduledOccurrenceCandidate, ScheduledOccurrenceRepository,
    ScheduledOccurrenceResolution, ScheduledRunIdGenerator,
};
use crate::RepositoryError;

const SCHEDULED_AT: i64 = 1_000_000;

#[test]
fn replay_returns_the_original_application_admission_after_definition_changes() {
    let handler = AdmitScheduledOccurrenceHandler::new(
        MemoryScheduledOccurrenceRepository::default(),
        IncrementingRunIdGenerator::default(),
    );
    let task = scheduled_task();
    let first = handler
        .handle(&task, SCHEDULED_AT, SCHEDULED_AT)
        .expect("admit first occurrence");
    let ScheduledOccurrenceResolution::Admitted(first_run) = first else {
        panic!("first delivery must admit a run");
    };

    let mut edited = task;
    edited.revision = 2;
    edited.prompt = "New prompt".to_string();
    let replay = handler
        .handle(&edited, SCHEDULED_AT, SCHEDULED_AT + 1)
        .expect("replay occurrence");

    assert_eq!(
        replay,
        ScheduledOccurrenceResolution::ExistingRun(first_run.clone())
    );
    assert_eq!(first_run.snapshot.revision, 1);
    assert_eq!(first_run.snapshot.prompt, "Run the quality gate");
}

#[test]
fn ten_minute_boundary_admits_but_the_next_millisecond_records_missed() {
    let due_handler = AdmitScheduledOccurrenceHandler::new(
        MemoryScheduledOccurrenceRepository::default(),
        IncrementingRunIdGenerator::default(),
    );
    assert!(matches!(
        due_handler
            .handle(&scheduled_task(), SCHEDULED_AT, SCHEDULED_AT + 600_000)
            .expect("admit boundary occurrence"),
        ScheduledOccurrenceResolution::Admitted(_)
    ));

    let missed_handler = AdmitScheduledOccurrenceHandler::new(
        MemoryScheduledOccurrenceRepository::default(),
        PanicRunIdGenerator,
    );
    assert_eq!(
        missed_handler
            .handle(&scheduled_task(), SCHEDULED_AT, SCHEDULED_AT + 600_001,)
            .expect("record late occurrence"),
        ScheduledOccurrenceResolution::Missed(ora_domain::ScheduledMissedOccurrence {
            occurrence: ScheduledOccurrence {
                task_id: ScheduledTaskId::new("scheduled-task-1"),
                scheduled_at: SCHEDULED_AT,
            },
            reason: ScheduledMissReason::LateTrigger,
            observed_at: SCHEDULED_AT + 600_001,
        })
    );
}

#[test]
fn early_observation_does_not_persist_or_allocate_a_run() {
    let handler = AdmitScheduledOccurrenceHandler::new(
        MemoryScheduledOccurrenceRepository::default(),
        PanicRunIdGenerator,
    );
    assert_eq!(
        handler
            .handle(&scheduled_task(), SCHEDULED_AT, SCHEDULED_AT - 1)
            .expect("ignore early observation"),
        ScheduledOccurrenceResolution::NotDue
    );
}

#[derive(Default)]
struct MemoryScheduledOccurrenceRepository {
    candidate: Mutex<Option<ScheduledOccurrenceCandidate>>,
}

impl ScheduledOccurrenceRepository for MemoryScheduledOccurrenceRepository {
    fn resolve_scheduled_occurrence(
        &self,
        candidate: ScheduledOccurrenceCandidate,
    ) -> Result<ScheduledOccurrenceResolution, RepositoryError> {
        let mut stored = self.candidate.lock().expect("lock occurrence");
        if let Some(existing) = stored.as_ref() {
            return Ok(match existing {
                ScheduledOccurrenceCandidate::Run(run) => {
                    ScheduledOccurrenceResolution::ExistingRun(run.clone())
                }
                ScheduledOccurrenceCandidate::Missed(missed) => {
                    ScheduledOccurrenceResolution::ExistingMissed(missed.clone())
                }
            });
        }
        let resolution = match &candidate {
            ScheduledOccurrenceCandidate::Run(run) => {
                ScheduledOccurrenceResolution::Admitted(run.clone())
            }
            ScheduledOccurrenceCandidate::Missed(missed) => {
                ScheduledOccurrenceResolution::Missed(missed.clone())
            }
        };
        *stored = Some(candidate);
        Ok(resolution)
    }
}

#[derive(Default)]
struct IncrementingRunIdGenerator {
    next: Mutex<u64>,
}

impl ScheduledRunIdGenerator for IncrementingRunIdGenerator {
    fn generate_scheduled_run_id(&self) -> ScheduledRunId {
        let mut next = self.next.lock().expect("lock generator");
        *next += 1;
        ScheduledRunId::new(format!("run-{next}"))
    }
}

struct PanicRunIdGenerator;

impl ScheduledRunIdGenerator for PanicRunIdGenerator {
    fn generate_scheduled_run_id(&self) -> ScheduledRunId {
        panic!("missed and early occurrences must not allocate a run")
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
