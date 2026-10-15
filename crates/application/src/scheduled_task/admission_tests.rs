use std::sync::Mutex;

use ora_domain::{
    AgentRef, AuditFields, ProjectId, ScheduledOccurrence, ScheduledRun, ScheduledRunId,
    ScheduledTask, ScheduledTaskApproval, ScheduledTaskExecutionLocation, ScheduledTaskId,
    ScheduledTaskSchedule,
};
use pretty_assertions::assert_eq;

use super::{
    AdmitScheduledOccurrenceHandler, ScheduledRunAdmission, ScheduledRunIdGenerator,
    ScheduledRunRepository,
};
use crate::RepositoryError;

#[test]
fn replay_returns_the_original_application_admission_after_definition_changes() {
    let handler = AdmitScheduledOccurrenceHandler::new(
        MemoryScheduledRunRepository::default(),
        IncrementingRunIdGenerator::default(),
    );
    let task = scheduled_task();
    let first = handler
        .handle(&task, 100, 101)
        .expect("admit first occurrence");
    assert!(matches!(first, ScheduledRunAdmission::Admitted(_)));

    let mut edited = task;
    edited.revision = 2;
    edited.prompt = "New prompt".to_string();
    let replay = handler
        .handle(&edited, 100, 102)
        .expect("replay occurrence");

    assert_eq!(replay, ScheduledRunAdmission::Existing(first.run().clone()));
    assert_eq!(replay.run().snapshot.revision, 1);
    assert_eq!(replay.run().snapshot.prompt, "Run the quality gate");
}

#[derive(Default)]
struct MemoryScheduledRunRepository {
    run: Mutex<Option<ScheduledRun>>,
}

impl ScheduledRunRepository for MemoryScheduledRunRepository {
    fn admit_scheduled_run(
        &self,
        candidate: ScheduledRun,
    ) -> Result<ScheduledRunAdmission, RepositoryError> {
        let mut run = self.run.lock().expect("lock run");
        if let Some(existing) = run.as_ref() {
            return Ok(ScheduledRunAdmission::Existing(existing.clone()));
        }
        *run = Some(candidate.clone());
        Ok(ScheduledRunAdmission::Admitted(candidate))
    }

    fn find_scheduled_run(
        &self,
        occurrence: &ScheduledOccurrence,
    ) -> Result<Option<ScheduledRun>, RepositoryError> {
        Ok(self
            .run
            .lock()
            .expect("lock run")
            .as_ref()
            .filter(|run| &run.occurrence == occurrence)
            .cloned())
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
