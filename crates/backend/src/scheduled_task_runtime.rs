use std::collections::HashMap;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent_client_protocol_schema::v1::{ContentBlock, TextContent};
use chrono::{TimeZone, Utc};
use cron::Schedule;
use ora_application::{
    AdmitScheduledOccurrenceHandler, Clock, RecordOfflineMissedOccurrenceHandler,
    ScheduledOccurrenceResolution, ScheduledRunLifecycleRepository, ScheduledTaskRepository,
    UuidScheduledRunIdGenerator,
};
use ora_contracts::{
    CreateTaskRequest, GetTaskWorkspaceRequest, PromptSessionEvent, PromptSessionRequest,
    StartSessionRequest,
};
use ora_db::{RepositoryPool, SqliteScheduledRunRepository, SqliteScheduledTaskRepository};
use ora_domain::{
    MAX_SCHEDULED_OCCURRENCE_LATENESS_MILLIS, ScheduledRun, ScheduledRunFailureReason,
    ScheduledRunTransition, ScheduledTask, ScheduledTaskExecutionLocation, ScheduledTaskId,
    ScheduledTaskSchedule, ScheduledTaskWeekday, SessionId,
};
use ora_logging::ora_warn;
use ora_scheduler::{BoxFuture, CronHandle, DelayHandle, Job, Scheduler};

use crate::{Sessions, TaskApi, clock::SystemClock};

/// Owns the in-process handles for persisted local scheduled-task definitions.
///
/// Definitions and run facts remain authoritative in SQLite. The handles only mirror the
/// currently live process, so startup can safely reconstruct them and every trigger re-reads the
/// definition before admitting work.
pub(super) struct ScheduledTaskRuntime {
    scheduler: Scheduler,
    tasks: SqliteScheduledTaskRepository,
    runs: SqliteScheduledRunRepository,
    executor: DesktopScheduledRunExecutor,
    clock: SystemClock,
    handles: Mutex<HashMap<String, ScheduledTaskHandle>>,
}

#[allow(dead_code)] // Ownership, and therefore Drop-based cancellation, is the only use.
enum ScheduledTaskHandle {
    Cron(CronHandle),
    Delay(DelayHandle),
}

impl ScheduledTaskRuntime {
    pub(super) fn new(
        pool: RepositoryPool,
        scheduler: Scheduler,
        tasks_api: Arc<TaskApi>,
        sessions: Arc<Sessions>,
        clock: SystemClock,
    ) -> Arc<Self> {
        let runtime = Arc::new(Self {
            scheduler,
            tasks: SqliteScheduledTaskRepository::new(pool.clone()),
            runs: SqliteScheduledRunRepository::new(pool.clone()),
            executor: DesktopScheduledRunExecutor {
                tasks: tasks_api,
                sessions,
                runs: SqliteScheduledRunRepository::new(pool),
                clock,
            },
            clock,
            handles: Mutex::new(HashMap::new()),
        });
        runtime.restore();
        runtime
    }

    /// Rebuilds only definitions whose persisted approval and frozen execution inputs permit
    /// local execution. Legacy definitions without the new frozen fields remain inert.
    fn restore(self: &Arc<Self>) {
        match self.tasks.list_scheduled_tasks() {
            Ok(tasks) => {
                for task in tasks {
                    self.recover_offline_occurrences(&task);
                    self.reconcile(task);
                }
            }
            Err(error) => {
                ora_warn!(error = %error, "scheduled task recovery could not list definitions")
            }
        }
    }

    fn recover_offline_occurrences(self: &Arc<Self>, task: &ScheduledTask) {
        if !is_schedulable(task) {
            return;
        }
        let observed_at = self.clock.now_timestamp_millis();
        let occurrences =
            recent_scheduled_occurrences(task, observed_at, self.scheduler.timezone());
        let Some(&latest) = occurrences.last() else {
            return;
        };
        if let Err(error) = RecordOfflineMissedOccurrenceHandler::new(self.runs.clone()).handle(
            task,
            occurrences.iter().copied(),
            observed_at,
        ) {
            ora_warn!(task_id = %task.id, error = %error, "scheduled task offline recovery failed");
            return;
        }
        if observed_at.saturating_sub(latest) <= MAX_SCHEDULED_OCCURRENCE_LATENESS_MILLIS {
            let runtime = Arc::clone(self);
            let task_id = task.id.clone();
            match self.scheduler.schedule_after(Duration::ZERO, async move {
                runtime.trigger(task_id, latest).await;
            }) {
                Ok(handle) => handle.detach(),
                Err(error) => {
                    ora_warn!(task_id = %task.id, error = %error, "scheduled task live recovery could not be registered")
                }
            }
        }
    }

    pub(super) fn reconcile(self: &Arc<Self>, task: ScheduledTask) {
        self.cancel(&task.id);
        if !is_schedulable(&task) {
            return;
        }

        let key = task.id.to_string();
        let handle = match &task.schedule {
            ScheduledTaskSchedule::EveryHours { .. } => self.schedule_hourly(task.clone()),
            ScheduledTaskSchedule::Daily { .. } | ScheduledTaskSchedule::Weekly { .. } => {
                self.schedule_calendar(task.clone())
            }
        };
        match handle {
            Ok(Some(handle)) => {
                let mut handles = self
                    .handles
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                handles.insert(key, handle);
            }
            Ok(None) => {}
            Err(error) => {
                ora_warn!(task_id = %task.id, error = %error, "failed to register scheduled task")
            }
        }
    }

    pub(super) fn cancel(&self, task_id: &ScheduledTaskId) {
        let mut handles = self
            .handles
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _ = handles.remove(task_id.as_ref());
    }

    fn schedule_hourly(
        self: &Arc<Self>,
        task: ScheduledTask,
    ) -> Result<Option<ScheduledTaskHandle>, ora_scheduler::SchedulerError> {
        let Some(anchor_at) = task.schedule_anchor_at else {
            return Ok(None);
        };
        let now = self.clock.now_timestamp_millis();
        let Some(scheduled_at) = task.schedule.next_hourly_occurrence_after(anchor_at, now) else {
            return Ok(None);
        };
        let delay = Duration::from_millis(scheduled_at.saturating_sub(now).max(0) as u64);
        let runtime = Arc::clone(self);
        let task_id = task.id.clone();
        self.scheduler
            .schedule_after(delay, async move {
                runtime.trigger(task_id, scheduled_at).await;
            })
            .map(ScheduledTaskHandle::Delay)
            .map(Some)
    }

    fn schedule_calendar(
        self: &Arc<Self>,
        task: ScheduledTask,
    ) -> Result<Option<ScheduledTaskHandle>, ora_scheduler::SchedulerError> {
        let schedule =
            calendar_cron_expression(&task.schedule).expect("calendar schedule selected");
        let job = CalendarScheduledTaskJob {
            name: format!("scheduled-task:{}", task.id),
            schedule,
            runtime: Arc::clone(self),
            task_id: task.id,
        };
        self.scheduler
            .schedule_cron(job)
            .map(ScheduledTaskHandle::Cron)
            .map(Some)
    }

    async fn trigger(self: Arc<Self>, task_id: ScheduledTaskId, scheduled_at: i64) {
        let task = match self.tasks.find_scheduled_task(&task_id) {
            Ok(Some(task)) => task,
            Ok(None) => return,
            Err(error) => {
                ora_warn!(task_id = %task_id, error = %error, "scheduled task trigger could not reload definition");
                return;
            }
        };
        if !is_schedulable(&task) {
            self.cancel(&task_id);
            return;
        }

        // Re-arm the one-shot timer before the run can spend time provisioning or prompting.
        // Admission remains the concurrency boundary, so a long-running occurrence cannot shift
        // the persisted hourly cadence or cause a later tick to overlap unsafely.
        if matches!(task.schedule, ScheduledTaskSchedule::EveryHours { .. }) {
            self.reconcile(task.clone());
        }

        let observed_at = self.clock.now_timestamp_millis();
        let admission = AdmitScheduledOccurrenceHandler::new(
            self.runs.clone(),
            UuidScheduledRunIdGenerator::new(),
        )
        .handle(&task, scheduled_at, observed_at);
        match admission {
            Ok(ScheduledOccurrenceResolution::Admitted(run)) => self.executor.execute(run).await,
            Ok(_) => {}
            Err(error) => {
                ora_warn!(task_id = %task_id, error = %error, "scheduled task occurrence admission failed")
            }
        }
    }
}

struct CalendarScheduledTaskJob {
    name: String,
    schedule: String,
    runtime: Arc<ScheduledTaskRuntime>,
    task_id: ScheduledTaskId,
}

impl Job for CalendarScheduledTaskJob {
    fn name(&self) -> &str {
        &self.name
    }

    fn schedule(&self) -> &str {
        &self.schedule
    }

    fn run(&self) -> BoxFuture<'_> {
        let runtime = Arc::clone(&self.runtime);
        let task_id = self.task_id.clone();
        Box::pin(async move {
            let scheduled_at = runtime.clock.now_timestamp_millis();
            runtime.trigger(task_id, scheduled_at).await;
        })
    }
}

/// Executes an admitted local occurrence through the existing task worktree and session APIs.
struct DesktopScheduledRunExecutor {
    tasks: Arc<TaskApi>,
    sessions: Arc<Sessions>,
    runs: SqliteScheduledRunRepository,
    clock: SystemClock,
}

impl DesktopScheduledRunExecutor {
    async fn execute(&self, run: ScheduledRun) {
        let Some(base_branch) = run
            .snapshot
            .worktree_base_branch
            .as_deref()
            .filter(|branch| !branch.trim().is_empty())
        else {
            self.fail(&run, ScheduledRunFailureReason::ApprovalPolicyInvalid);
            return;
        };
        let created = match self.tasks.create(CreateTaskRequest {
            project_id: run.snapshot.project_id.to_string(),
            title: format!("Scheduled run {}", run.id),
            base_branch: Some(base_branch.to_string()),
        }) {
            Ok(created) => created,
            Err(error) => {
                ora_warn!(run_id = %run.id, error = %error, "scheduled run worktree creation failed");
                self.fail(&run, ScheduledRunFailureReason::WorktreeFailed);
                return;
            }
        };
        let workspace = match self.tasks.workspace(GetTaskWorkspaceRequest {
            task_id: created.task.id.clone(),
        }) {
            Ok(workspace) => workspace.workspace,
            Err(error) => {
                ora_warn!(run_id = %run.id, error = %error, "scheduled run workspace lookup failed");
                self.fail(&run, ScheduledRunFailureReason::WorktreeFailed);
                return;
            }
        };
        let started = match self
            .sessions
            .start(StartSessionRequest {
                workspace_id: created.task.workspace_id,
                agent_ref: run.snapshot.agent_ref.to_string(),
                model: run.snapshot.model.clone(),
            })
            .await
        {
            Ok(started) => started,
            Err(error) => {
                ora_warn!(run_id = %run.id, error = %error, "scheduled run session start failed");
                self.fail(&run, ScheduledRunFailureReason::TargetUnavailable);
                return;
            }
        };
        let session_id = SessionId::new(started.session.id);
        if !self.transition(
            &run,
            ScheduledRunTransition::Start {
                session_id: session_id.clone(),
                worktree_path: workspace.root_path,
            },
        ) {
            return;
        }
        let mut stream = match self
            .sessions
            .prompt(PromptSessionRequest {
                session_id: session_id.to_string(),
                prompt: vec![ContentBlock::Text(TextContent::new(
                    run.snapshot.prompt.clone(),
                ))],
                record_prompt: None,
                model: None,
            })
            .await
        {
            Ok(stream) => stream,
            Err(error) => {
                ora_warn!(run_id = %run.id, error = %error, "scheduled run prompt start failed");
                self.fail(&run, ScheduledRunFailureReason::ExecutionError);
                return;
            }
        };
        while let Some(event) = stream.recv().await {
            match event {
                Ok(PromptSessionEvent::Completed { .. }) => {
                    self.transition(&run, ScheduledRunTransition::Succeed);
                    return;
                }
                Ok(_) => {}
                Err(error) => {
                    ora_warn!(run_id = %run.id, error = %error, "scheduled run prompt failed");
                    self.fail(&run, ScheduledRunFailureReason::ExecutionError);
                    return;
                }
            }
        }
        self.fail(&run, ScheduledRunFailureReason::ExecutionError);
    }

    fn fail(&self, run: &ScheduledRun, reason: ScheduledRunFailureReason) {
        let _ = self.transition(run, ScheduledRunTransition::Fail { reason });
    }

    fn transition(&self, run: &ScheduledRun, transition: ScheduledRunTransition) -> bool {
        match self.runs.transition_scheduled_run(
            &run.id,
            transition,
            self.clock.now_timestamp_millis(),
        ) {
            Ok(ora_application::ScheduledRunTransitionResolution::Updated(_)) => true,
            Ok(other) => {
                ora_warn!(run_id = %run.id, result = ?other, "scheduled run lifecycle transition was not accepted");
                false
            }
            Err(error) => {
                ora_warn!(run_id = %run.id, error = %error, "scheduled run lifecycle transition failed");
                false
            }
        }
    }
}

/// Returns the latest two planned times at or before startup. The offline-recovery handler needs
/// both values: if the newest point is still in its live lateness window, the immediately prior
/// point may nevertheless need an `app_not_running` fact.
fn recent_scheduled_occurrences(
    task: &ScheduledTask,
    observed_at: i64,
    timezone: chrono_tz::Tz,
) -> Vec<i64> {
    match &task.schedule {
        ScheduledTaskSchedule::EveryHours { interval } => {
            let Some(anchor_at) = task.schedule_anchor_at else {
                return Vec::new();
            };
            let interval_millis = i64::from(*interval).saturating_mul(60 * 60 * 1_000);
            if interval_millis <= 0 {
                return Vec::new();
            }
            // The enable-time anchor defines alignment; it is not itself an occurrence. This
            // mirrors `next_hourly_occurrence_after`, whose first result is anchor + interval.
            let first = anchor_at.saturating_add(interval_millis);
            if observed_at < first {
                return Vec::new();
            }
            let latest = first.saturating_add(
                observed_at
                    .saturating_sub(first)
                    .div_euclid(interval_millis)
                    .saturating_mul(interval_millis),
            );
            let previous = latest.saturating_sub(interval_millis);
            if previous >= anchor_at {
                vec![previous, latest]
            } else {
                vec![latest]
            }
        }
        ScheduledTaskSchedule::Daily { .. } | ScheduledTaskSchedule::Weekly { .. } => {
            let Some(expression) = calendar_cron_expression(&task.schedule) else {
                return Vec::new();
            };
            let Ok(schedule) = Schedule::from_str(&expression) else {
                return Vec::new();
            };
            let start_at = task.audit_fields.updated_at.min(observed_at);
            let Some(start) = Utc
                .timestamp_millis_opt(start_at)
                .single()
                .map(|instant| instant.with_timezone(&timezone))
            else {
                return Vec::new();
            };
            let Some(observed) = Utc
                .timestamp_millis_opt(observed_at)
                .single()
                .map(|instant| instant.with_timezone(&timezone))
            else {
                return Vec::new();
            };
            let mut recent = Vec::with_capacity(2);
            for occurrence in schedule.after(&start) {
                if occurrence > observed {
                    break;
                }
                let millis = occurrence.timestamp_millis();
                if recent.len() == 2 {
                    recent.remove(0);
                }
                recent.push(millis);
            }
            recent
        }
    }
}

fn is_schedulable(task: &ScheduledTask) -> bool {
    task.enabled
        && task.execution_location == ScheduledTaskExecutionLocation::Local
        && task.local_activation_allowed()
        && task
            .worktree_base_branch
            .as_deref()
            .map_or(false, |branch| !branch.trim().is_empty())
        && match task.schedule {
            ScheduledTaskSchedule::EveryHours { .. } => task.schedule_anchor_at.is_some(),
            ScheduledTaskSchedule::Daily { .. } | ScheduledTaskSchedule::Weekly { .. } => true,
        }
}

fn calendar_cron_expression(schedule: &ScheduledTaskSchedule) -> Option<String> {
    match schedule {
        ScheduledTaskSchedule::Daily { hour, minute } => Some(format!("0 {minute} {hour} * * *")),
        ScheduledTaskSchedule::Weekly {
            weekdays,
            hour,
            minute,
        } => Some(format!(
            "0 {minute} {hour} * * {}",
            weekdays
                .iter()
                .map(weekday_cron_name)
                .collect::<Vec<_>>()
                .join(",")
        )),
        ScheduledTaskSchedule::EveryHours { .. } => None,
    }
}

fn weekday_cron_name(day: &ScheduledTaskWeekday) -> &'static str {
    match day {
        ScheduledTaskWeekday::Monday => "MON",
        ScheduledTaskWeekday::Tuesday => "TUE",
        ScheduledTaskWeekday::Wednesday => "WED",
        ScheduledTaskWeekday::Thursday => "THU",
        ScheduledTaskWeekday::Friday => "FRI",
        ScheduledTaskWeekday::Saturday => "SAT",
        ScheduledTaskWeekday::Sunday => "SUN",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use ora_domain::{AgentRef, AuditFields, ProjectId, ScheduledTaskApproval, ScheduledTaskId};

    use super::{calendar_cron_expression, is_schedulable, recent_scheduled_occurrences};
    use ora_domain::{
        ScheduledTask, ScheduledTaskExecutionLocation, ScheduledTaskSchedule, ScheduledTaskWeekday,
    };

    fn approved_task(schedule: ScheduledTaskSchedule) -> ScheduledTask {
        let mut task = ScheduledTask::new(
            ScheduledTaskId::new("scheduled-task-1"),
            "Quality report",
            true,
            schedule,
            ScheduledTaskExecutionLocation::Local,
            ProjectId::new("project-1"),
            AgentRef::parse("ora/agent").expect("agent ref"),
            Some("default".to_string()),
            "Run the quality gate",
            Some(ScheduledTaskApproval::full_auto_in_worktree(10)),
            AuditFields::new(10, 10, false),
        )
        .expect("task");
        task.freeze_worktree_base_branch("main");
        task
    }

    #[test]
    fn only_recovered_definitions_with_frozen_execution_inputs_are_schedulable() {
        let mut hourly =
            approved_task(ScheduledTaskSchedule::every_hours(4).expect("hourly schedule"));
        assert!(!is_schedulable(&hourly));

        hourly.reanchor_on_enable(10);
        assert!(is_schedulable(&hourly));

        hourly.worktree_base_branch = None;
        assert!(!is_schedulable(&hourly));
    }

    #[test]
    fn calendar_schedules_preserve_the_configured_local_wall_clock_shape() {
        let daily = ScheduledTaskSchedule::daily(9, 5).expect("daily schedule");
        assert_eq!(
            calendar_cron_expression(&daily).as_deref(),
            Some("0 5 9 * * *")
        );

        let weekly = ScheduledTaskSchedule::weekly(
            BTreeSet::from([ScheduledTaskWeekday::Monday, ScheduledTaskWeekday::Friday]),
            18,
            30,
        )
        .expect("weekly schedule");
        assert_eq!(
            calendar_cron_expression(&weekly).as_deref(),
            Some("0 30 18 * * MON,FRI")
        );
    }

    #[test]
    fn offline_recovery_keeps_the_latest_expired_hourly_point_and_live_candidate() {
        let mut hourly =
            approved_task(ScheduledTaskSchedule::every_hours(2).expect("hourly schedule"));
        hourly.reanchor_on_enable(1_000);

        assert_eq!(
            recent_scheduled_occurrences(&hourly, 14_401_000, chrono_tz::UTC),
            vec![7_201_000, 14_401_000]
        );
    }
}
