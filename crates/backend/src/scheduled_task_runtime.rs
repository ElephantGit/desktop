use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent_client_protocol_schema::v1::{ContentBlock, StopReason, TextContent};
use ora_application::{
    AdmitScheduledOccurrenceHandler, Clock, RecordOfflineMissedOccurrenceHandler,
    ScheduledOccurrenceResolution, ScheduledRunLifecycleRepository, ScheduledTaskRepository,
    UuidScheduledRunIdGenerator,
};
use ora_contracts::{
    PromptSessionEvent, PromptSessionRequest, StartSessionRequest, StopSessionRequest,
};
use ora_db::{RepositoryPool, SqliteScheduledRunRepository, SqliteScheduledTaskRepository};
use ora_domain::{
    MAX_SCHEDULED_OCCURRENCE_LATENESS_MILLIS, ScheduledRun, ScheduledRunFailureReason,
    ScheduledRunStatus, ScheduledRunTransition, ScheduledTask, ScheduledTaskId,
    ScheduledTaskSchedule, SessionId, TaskId, parse_scheduled_run_verdict,
};
use ora_history::{final_assistant_message, read_session_history};
use ora_logging::ora_warn;
use ora_scheduler::{BoxFuture, CronHandle, DelayHandle, Job, Scheduler};

use crate::app_event::AppEventPublisher;
use crate::task::ScheduledRunWorktreeCommand;
use crate::{Sessions, TaskApi, clock::SystemClock};

use super::calendar::{
    calendar_cron_expression, is_schedulable, latest_calendar_occurrence_at_or_before,
    recent_scheduled_occurrences,
};
use super::worktree::ScheduledWorktreeJanitor;

const SCHEDULED_RUN_TIMEOUT: Duration = Duration::from_secs(60 * 60);

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
        events: AppEventPublisher,
    ) -> Arc<Self> {
        let janitor = ScheduledWorktreeJanitor::new(
            tasks_api.clone(),
            pool.clone(),
            tasks_api.relative_path_base().to_path_buf(),
        );
        let runtime = Arc::new(Self {
            scheduler,
            tasks: SqliteScheduledTaskRepository::new(pool.clone()),
            runs: SqliteScheduledRunRepository::new(pool.clone()),
            executor: DesktopScheduledRunExecutor {
                tasks: tasks_api,
                sessions,
                runs: SqliteScheduledRunRepository::new(pool),
                clock,
                janitor,
                events,
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
        let interrupted_at = self.clock.now_timestamp_millis();
        if let Err(error) = self.runs.fail_interrupted_scheduled_runs(interrupted_at) {
            ora_warn!(error = %error, "scheduled task startup recovery could not finish interrupted runs");
        }
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
        // A previous process may have finished runs without cleaning their worktrees, or left
        // more kept worktrees than the retention bound. The sweep is best-effort: a lost pass
        // is retried by the next finished run.
        let janitor = self.executor.janitor.clone();
        match self.scheduler.schedule_after(Duration::ZERO, async move {
            janitor.enforce_retention().await;
        }) {
            Ok(handle) => handle.detach(),
            Err(error) => {
                ora_warn!(error = %error, "scheduled task worktree retention sweep could not be queued")
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
        // Startup recovery facts (offline misses, interrupted runs) are recorded before any
        // webview can subscribe to the event stream, so publishing them here would always
        // drop. The frontend surfaces these persisted facts on stream `ready` instead; the
        // live trigger path below publishes while subscribers exist.
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
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
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
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
        let task_id = task.id;
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
        let Some(schedule) = calendar_cron_expression(&task.schedule) else {
            return Ok(None);
        };
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
            // A late live observation persists a missed fact; announce it once so the user
            // learns why nothing ran. Replays (ExistingMissed) never re-notify.
            Ok(ScheduledOccurrenceResolution::Missed(missed)) => {
                self.executor
                    .events
                    .try_publish(ora_contracts::AppEvent::ScheduledTask(
                        ora_contracts::ScheduledTaskEvent::OccurrenceMissed {
                            task_id: missed.occurrence.task_id.to_string(),
                            scheduled_at: missed.occurrence.scheduled_at,
                            reason: contract_miss_reason(missed.reason),
                        },
                    ));
            }
            Ok(_) => {}
            Err(error) => {
                ora_warn!(task_id = %task_id, error = %error, "scheduled task occurrence admission failed")
            }
        }
    }

    fn enqueue_calendar_trigger(self: &Arc<Self>, task_id: ScheduledTaskId) {
        let runtime = Arc::clone(self);
        match self.scheduler.schedule_after(Duration::ZERO, async move {
            runtime.trigger_calendar(task_id).await;
        }) {
            Ok(handle) => handle.detach(),
            Err(error) => {
                ora_warn!(error = %error, "scheduled task calendar trigger could not be queued")
            }
        }
    }

    async fn trigger_calendar(self: Arc<Self>, task_id: ScheduledTaskId) {
        let task = match self.tasks.find_scheduled_task(&task_id) {
            Ok(Some(task)) => task,
            Ok(None) => return,
            Err(error) => {
                ora_warn!(task_id = %task_id, error = %error, "scheduled task calendar trigger could not reload definition");
                return;
            }
        };
        let observed_at = self.clock.now_timestamp_millis();
        let Some(scheduled_at) = latest_calendar_occurrence_at_or_before(
            &task.schedule,
            observed_at,
            self.scheduler.timezone(),
        ) else {
            ora_warn!(task_id = %task_id, "scheduled task calendar trigger had no due occurrence");
            return;
        };
        self.trigger(task_id, scheduled_at).await;
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
            // A cron callback only queues execution. The queued path derives the stable planned
            // tick before admission, so a long-running agent cannot stall later cron ticks.
            runtime.enqueue_calendar_trigger(task_id);
        })
    }
}

/// Executes an admitted local occurrence through the existing task worktree and session APIs.
struct DesktopScheduledRunExecutor {
    tasks: Arc<TaskApi>,
    sessions: Arc<Sessions>,
    runs: SqliteScheduledRunRepository,
    clock: SystemClock,
    janitor: ScheduledWorktreeJanitor,
    events: AppEventPublisher,
}

impl DesktopScheduledRunExecutor {
    async fn execute(&self, run: ScheduledRun) {
        let Some(base_branch) = run
            .snapshot
            .worktree_base_branch
            .as_deref()
            .filter(|branch| !branch.trim().is_empty())
        else {
            self.fail(&run, ScheduledRunFailureReason::WorktreeFailed)
                .await;
            return;
        };
        let created = match self
            .tasks
            .create_for_scheduled_run(ScheduledRunWorktreeCommand {
                project_id: run.snapshot.project_id.to_string(),
                title: format!("Scheduled run {}", run.id),
                base_branch: base_branch.to_string(),
                run_id: run.id.to_string(),
            }) {
            Ok(created) => created,
            Err(error) => {
                ora_warn!(run_id = %run.id, error = %error, "scheduled run worktree creation failed");
                self.fail(&run, ScheduledRunFailureReason::WorktreeFailed)
                    .await;
                return;
            }
        };
        // The ownership commit already bound this task and path to the run row in
        // the same transaction that created it, so no crash after this point can
        // leave the freshly provisioned worktree without a durable owner.
        let worktree_task_id = TaskId::new(created.task.id.clone());
        let worktree_path = created.worktree_path;
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
                self.finish(
                    &run,
                    Some(&worktree_task_id),
                    None,
                    ScheduledRunTransition::Fail {
                        reason: ScheduledRunFailureReason::TargetUnavailable,
                    },
                )
                .await;
                return;
            }
        };
        let session_id = SessionId::new(started.session.id);
        if !self.transition(
            &run,
            ScheduledRunTransition::Start {
                session_id: session_id.clone(),
                worktree_path: worktree_path.clone(),
                worktree_task_id: worktree_task_id.clone(),
            },
        ) {
            // The run row already reached a terminal state (startup recovery won the race),
            // so the freshly provisioned worktree is discarded directly. Stop the session
            // first: the normal task cascade refuses to remove an active workspace.
            self.stop_session_quietly(&session_id).await;
            self.janitor
                .run_finished(&run, &worktree_task_id, None)
                .await;
            return;
        }
        self.events
            .try_publish(ora_contracts::AppEvent::ScheduledTask(
                ora_contracts::ScheduledTaskEvent::RunStarted {
                    task_id: run.occurrence.task_id.to_string(),
                    session_id: session_id.to_string(),
                    worktree_task_id: worktree_task_id.to_string(),
                    project_id: run.snapshot.project_id.to_string(),
                },
            ));
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
                self.stop_session_quietly(&session_id).await;
                self.finish(
                    &run,
                    Some(&worktree_task_id),
                    Some(&worktree_path),
                    ScheduledRunTransition::Fail {
                        reason: ScheduledRunFailureReason::ExecutionError,
                    },
                )
                .await;
                return;
            }
        };
        let timeout = tokio::time::sleep(SCHEDULED_RUN_TIMEOUT);
        tokio::pin!(timeout);
        loop {
            tokio::select! {
                _ = &mut timeout => {
                    if let Err(error) = self.sessions.stop(StopSessionRequest {
                        session_id: session_id.to_string(),
                    }).await {
                        ora_warn!(run_id = %run.id, error = %error, "scheduled run timeout could not stop session");
                    }
                    self.finish(
                        &run,
                        Some(&worktree_task_id),
                        Some(&worktree_path),
                        ScheduledRunTransition::Fail {
                            reason: ScheduledRunFailureReason::Timeout,
                        },
                    )
                    .await;
                    return;
                }
                event = stream.recv() => match event {
                    Some(Ok(PromptSessionEvent::Completed { stop_reason, .. })) => {
                        // The prompt finished its turn, but the session stays attached and
                        // Running; stop it so the worktree cleanup path cannot hit the
                        // ActiveSession conflict on every terminal branch.
                        self.stop_session_quietly(&session_id).await;
                        let transition = scheduled_run_completion_transition(stop_reason);
                        if transition == ScheduledRunTransition::Succeed {
                            self.finish_succeeded(
                                &run,
                                &worktree_task_id,
                                &worktree_path,
                                &session_id,
                            )
                            .await;
                        } else {
                            self.finish(
                                &run,
                                Some(&worktree_task_id),
                                Some(&worktree_path),
                                transition,
                            )
                            .await;
                        }
                        return;
                    }
                    Some(Ok(_)) => {}
                    Some(Err(error)) => {
                        ora_warn!(run_id = %run.id, error = %error, "scheduled run prompt failed");
                        self.stop_session_quietly(&session_id).await;
                        self.finish(
                            &run,
                            Some(&worktree_task_id),
                            Some(&worktree_path),
                            ScheduledRunTransition::Fail {
                                reason: ScheduledRunFailureReason::ExecutionError,
                            },
                        )
                        .await;
                        return;
                    }
                    None => {
                        self.stop_session_quietly(&session_id).await;
                        self.finish(
                            &run,
                            Some(&worktree_task_id),
                            Some(&worktree_path),
                            ScheduledRunTransition::Fail {
                                reason: ScheduledRunFailureReason::ExecutionError,
                            },
                        )
                        .await;
                        return;
                    }
                },
            }
        }
    }

    /// Stops the run's session without failing the run: a failed stop only means the
    /// worktree cleanup will meet the ActiveSession conflict and be retried by the sweep.
    async fn stop_session_quietly(&self, session_id: &SessionId) {
        if let Err(error) = self
            .sessions
            .stop(StopSessionRequest {
                session_id: session_id.to_string(),
            })
            .await
        {
            ora_warn!(error = %error, "scheduled run could not stop its session before cleanup");
        }
    }

    /// Persists one terminal transition, announces it, and hands the worktree to the janitor.
    async fn finish(
        &self,
        run: &ScheduledRun,
        worktree_task_id: Option<&TaskId>,
        worktree_path: Option<&str>,
        transition: ScheduledRunTransition,
    ) {
        if !self.transition(run, transition.clone()) {
            return;
        }
        self.publish_finished(run, &transition);
        if let Some(worktree_task_id) = worktree_task_id {
            self.janitor
                .run_finished(run, worktree_task_id, worktree_path)
                .await;
        }
    }

    /// Finishes a run that ended its turn normally: persist `succeeded`, annotate the
    /// business verdict from the final Assistant turn, then publish and hand the worktree
    /// to the janitor. The verdict write happens after the terminal transition and before
    /// `RunFinished`, and is best-effort — a missing sentinel, an illegal value, an
    /// over-long detail, or a run that is no longer `succeeded` leaves it "not reported".
    async fn finish_succeeded(
        &self,
        run: &ScheduledRun,
        worktree_task_id: &TaskId,
        worktree_path: &str,
        session_id: &SessionId,
    ) {
        if !self.transition(run, ScheduledRunTransition::Succeed) {
            return;
        }
        self.annotate_verdict(run, session_id);
        self.publish_finished(run, &ScheduledRunTransition::Succeed);
        self.janitor
            .run_finished(run, worktree_task_id, Some(worktree_path))
            .await;
    }

    /// Parses the documented verdict sentinel out of the run's final Assistant turn and
    /// persists it once, without rewriting any execution-layer fact.
    fn annotate_verdict(&self, run: &ScheduledRun, session_id: &SessionId) {
        let history = match read_session_history(self.tasks.sessions_root(), session_id.as_ref()) {
            Ok(history) => history,
            Err(error) => {
                ora_warn!(run_id = %run.id, error = %error, "scheduled run verdict could not read session history");
                return;
            }
        };
        let Some(assistant) = final_assistant_message(&history) else {
            return;
        };
        let Some(annotation) = parse_scheduled_run_verdict(&assistant) else {
            return;
        };
        if let Err(error) = self.runs.annotate_scheduled_run_verdict(
            &run.id,
            annotation.verdict,
            annotation.detail.as_deref(),
            self.clock.now_timestamp_millis(),
        ) {
            ora_warn!(run_id = %run.id, error = %error, "scheduled run verdict annotation failed");
        }
    }

    /// Announces a terminal run state once the persisted fact is committed.
    fn publish_finished(&self, run: &ScheduledRun, transition: &ScheduledRunTransition) {
        let (status, failure_reason) = match transition {
            ScheduledRunTransition::Start { .. } => return,
            ScheduledRunTransition::Succeed => (ScheduledRunStatus::Succeeded, None),
            ScheduledRunTransition::Fail { reason } => (ScheduledRunStatus::Failed, Some(*reason)),
            ScheduledRunTransition::Cancel => (ScheduledRunStatus::Cancelled, None),
        };
        self.events
            .try_publish(ora_contracts::AppEvent::ScheduledTask(
                ora_contracts::ScheduledTaskEvent::RunFinished {
                    task_id: run.occurrence.task_id.to_string(),
                    run_id: run.id.to_string(),
                    status: contract_run_status(status),
                    failure_reason: failure_reason.map(contract_failure_reason),
                },
            ));
    }

    async fn fail(&self, run: &ScheduledRun, reason: ScheduledRunFailureReason) {
        self.finish(run, None, None, ScheduledRunTransition::Fail { reason })
            .await;
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

fn scheduled_run_completion_transition(stop_reason: StopReason) -> ScheduledRunTransition {
    match stop_reason {
        StopReason::EndTurn => ScheduledRunTransition::Succeed,
        StopReason::Cancelled => ScheduledRunTransition::Cancel,
        StopReason::MaxTokens | StopReason::MaxTurnRequests | StopReason::Refusal => {
            ScheduledRunTransition::Fail {
                reason: ScheduledRunFailureReason::ExecutionError,
            }
        }
        _ => ScheduledRunTransition::Fail {
            reason: ScheduledRunFailureReason::ExecutionError,
        },
    }
}

fn contract_run_status(status: ScheduledRunStatus) -> ora_contracts::ScheduledRunStatus {
    match status {
        ScheduledRunStatus::Accepted => ora_contracts::ScheduledRunStatus::Accepted,
        ScheduledRunStatus::Running => ora_contracts::ScheduledRunStatus::Running,
        ScheduledRunStatus::Succeeded => ora_contracts::ScheduledRunStatus::Succeeded,
        ScheduledRunStatus::Failed => ora_contracts::ScheduledRunStatus::Failed,
        ScheduledRunStatus::Cancelled => ora_contracts::ScheduledRunStatus::Cancelled,
        ScheduledRunStatus::SkippedBusy => ora_contracts::ScheduledRunStatus::SkippedBusy,
        ScheduledRunStatus::SkippedCapacity => ora_contracts::ScheduledRunStatus::SkippedCapacity,
    }
}

fn contract_failure_reason(
    reason: ScheduledRunFailureReason,
) -> ora_contracts::ScheduledRunFailureReason {
    match reason {
        ScheduledRunFailureReason::TargetUnavailable => {
            ora_contracts::ScheduledRunFailureReason::TargetUnavailable
        }
        ScheduledRunFailureReason::ModelUnavailable => {
            ora_contracts::ScheduledRunFailureReason::ModelUnavailable
        }
        ScheduledRunFailureReason::WorktreeFailed => {
            ora_contracts::ScheduledRunFailureReason::WorktreeFailed
        }
        ScheduledRunFailureReason::ApprovalPolicyInvalid => {
            ora_contracts::ScheduledRunFailureReason::ApprovalPolicyInvalid
        }
        ScheduledRunFailureReason::Timeout => ora_contracts::ScheduledRunFailureReason::Timeout,
        ScheduledRunFailureReason::ExecutionError => {
            ora_contracts::ScheduledRunFailureReason::ExecutionError
        }
    }
}

fn contract_miss_reason(
    reason: ora_domain::ScheduledMissReason,
) -> ora_contracts::ScheduledMissReason {
    match reason {
        ora_domain::ScheduledMissReason::AppNotRunning => {
            ora_contracts::ScheduledMissReason::AppNotRunning
        }
        ora_domain::ScheduledMissReason::LateTrigger => {
            ora_contracts::ScheduledMissReason::LateTrigger
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use agent_client_protocol_schema::v1::StopReason;
    use chrono::{TimeZone, Utc};
    use ora_domain::{
        AgentRef, AuditFields, ProjectId, ScheduledRunFailureReason, ScheduledRunTransition,
        ScheduledTaskApproval, ScheduledTaskId,
    };

    use super::super::calendar::{
        calendar_cron_expression, is_schedulable, latest_calendar_occurrence_at_or_before,
        recent_scheduled_occurrences,
    };
    use super::scheduled_run_completion_transition;
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

    #[test]
    fn calendar_live_trigger_and_recovery_use_the_same_planned_occurrence() {
        let observed_at = Utc
            .with_ymd_and_hms(2026, 10, 19, 9, 5, 0)
            .single()
            .expect("timestamp")
            .timestamp_millis();
        let scheduled_at = Utc
            .with_ymd_and_hms(2026, 10, 19, 9, 0, 0)
            .single()
            .expect("timestamp")
            .timestamp_millis();
        let mut daily = approved_task(ScheduledTaskSchedule::daily(9, 0).expect("daily"));
        daily.audit_fields =
            AuditFields::new(observed_at - 86_400_000, observed_at - 86_400_000, false);

        assert_eq!(
            latest_calendar_occurrence_at_or_before(&daily.schedule, observed_at, chrono_tz::UTC),
            Some(scheduled_at)
        );
        assert_eq!(
            recent_scheduled_occurrences(&daily, observed_at, chrono_tz::UTC).last(),
            Some(&scheduled_at)
        );
    }

    #[test]
    fn completion_stop_reasons_preserve_scheduled_run_terminal_state() {
        assert_eq!(
            scheduled_run_completion_transition(StopReason::EndTurn),
            ScheduledRunTransition::Succeed
        );
        assert_eq!(
            scheduled_run_completion_transition(StopReason::Cancelled),
            ScheduledRunTransition::Cancel
        );
        for reason in [
            StopReason::MaxTokens,
            StopReason::MaxTurnRequests,
            StopReason::Refusal,
        ] {
            assert_eq!(
                scheduled_run_completion_transition(reason),
                ScheduledRunTransition::Fail {
                    reason: ScheduledRunFailureReason::ExecutionError,
                }
            );
        }
    }
}
