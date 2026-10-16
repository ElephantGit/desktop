use ora_application::{
    RepositoryError, ScheduledOccurrenceCandidate, ScheduledOccurrenceRepository,
    ScheduledOccurrenceResolution, ScheduledRunLifecycleRepository,
    ScheduledRunTransitionResolution, ScheduledTaskHistoryRepository,
};
use ora_domain::{
    AgentRef, MAX_CONCURRENT_LOCAL_SCHEDULED_RUNS, MAX_SCHEDULED_TASK_HISTORY_ENTRIES, ProjectId,
    ScheduledMissReason, ScheduledMissedOccurrence, ScheduledOccurrence, ScheduledRun,
    ScheduledRunFailureReason, ScheduledRunId, ScheduledRunSnapshot, ScheduledRunStatus,
    ScheduledRunTransition, ScheduledTaskApproval, ScheduledTaskHistoryEntry, ScheduledTaskId,
    SessionId,
};
use rusqlite::{Row, Transaction, params};

use crate::repository::RepositoryPool;

/// Atomically resolves accepted and missed Scheduled Task occurrences through SQLite.
#[derive(Clone, Debug)]
pub struct SqliteScheduledRunRepository {
    pool: RepositoryPool,
}

impl SqliteScheduledRunRepository {
    /// Builds a Scheduled occurrence repository from the shared SQLite connection pool.
    pub fn new(pool: RepositoryPool) -> Self {
        Self { pool }
    }
}

impl ScheduledOccurrenceRepository for SqliteScheduledRunRepository {
    fn resolve_scheduled_occurrence(
        &self,
        candidate: ScheduledOccurrenceCandidate,
    ) -> Result<ScheduledOccurrenceResolution, RepositoryError> {
        let occurrence = candidate.occurrence().clone();
        self.pool
            .with_connection_mut(|connection| {
                let transaction = connection.transaction()?;
                let (outcome, missed_reason, observed_at) = candidate_columns(&candidate);
                let inserted = transaction.execute(
                    "INSERT INTO scheduled_occurrences (
                        task_id, scheduled_at, outcome, missed_reason, observed_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(task_id, scheduled_at) DO NOTHING",
                    params![
                        occurrence.task_id.as_ref(),
                        occurrence.scheduled_at,
                        outcome,
                        missed_reason,
                        observed_at,
                    ],
                )? > 0;

                let resolution = if inserted {
                    match &candidate {
                        ScheduledOccurrenceCandidate::Run(run) => {
                            let mut admitted = run.clone();
                            admitted.status =
                                admission_status(&transaction, &admitted.occurrence.task_id)?;
                            insert_scheduled_run(&transaction, &admitted)?;
                            ScheduledOccurrenceResolution::Admitted(admitted)
                        }
                        ScheduledOccurrenceCandidate::Missed(missed) => {
                            ScheduledOccurrenceResolution::Missed(missed.clone())
                        }
                    }
                } else {
                    load_existing_resolution(&transaction, &occurrence)?
                };
                if inserted {
                    prune_oldest_history(&transaction, &occurrence.task_id)?;
                }
                transaction.commit()?;
                Ok(resolution)
            })
            .map_err(repository_error_from_database)
    }
}

impl ScheduledTaskHistoryRepository for SqliteScheduledRunRepository {
    fn list_scheduled_task_history(
        &self,
        task_id: &ScheduledTaskId,
    ) -> Result<Vec<ScheduledTaskHistoryEntry>, RepositoryError> {
        self.pool
            .with_connection(|connection| {
                let mut statement = connection.prepare(
                    "SELECT o.task_id AS occurrence_task_id,
                            o.scheduled_at AS occurrence_scheduled_at,
                            o.outcome, o.missed_reason, o.observed_at,
                            r.id, r.task_id, r.scheduled_at, r.task_revision, r.project_id,
                            r.agent_ref, r.model, r.prompt, r.approval_policy,
                            r.approval_consented_at, r.status, r.session_id,
                            r.worktree_path, r.failure_reason, r.created_at, r.updated_at
                     FROM scheduled_occurrences o
                     LEFT JOIN scheduled_runs r
                       ON r.task_id = o.task_id AND r.scheduled_at = o.scheduled_at
                     WHERE o.task_id = ?1
                     ORDER BY o.scheduled_at DESC",
                )?;
                let mut rows = statement.query([task_id.as_ref()])?;
                let mut entries = Vec::new();
                while let Some(row) = rows.next()? {
                    entries.push(map_history_row(row)?);
                }
                Ok(entries)
            })
            .map_err(repository_error_from_database)
    }
}

impl ScheduledRunLifecycleRepository for SqliteScheduledRunRepository {
    fn transition_scheduled_run(
        &self,
        run_id: &ScheduledRunId,
        transition: ScheduledRunTransition,
        transitioned_at: i64,
    ) -> Result<ScheduledRunTransitionResolution, RepositoryError> {
        self.pool
            .with_connection_mut(|connection| {
                let transaction = connection.transaction()?;
                let resolution = match load_scheduled_run_by_id(&transaction, run_id)? {
                    None => ScheduledRunTransitionResolution::NotFound,
                    Some(mut run) => {
                        let previous_status = run.status;
                        if !run.transition(transition, transitioned_at) {
                            ScheduledRunTransitionResolution::InvalidState(run)
                        } else {
                            let updated = transaction.execute(
                                "UPDATE scheduled_runs
                                 SET status = ?1, session_id = ?2, worktree_path = ?3,
                                     failure_reason = ?4, updated_at = ?5
                                 WHERE id = ?6 AND status = ?7",
                                params![
                                    status_column(run.status),
                                    run.session_id.as_ref().map(AsRef::<str>::as_ref),
                                    run.worktree_path.as_deref(),
                                    run.failure_reason.map(failure_reason_column),
                                    run.updated_at,
                                    run.id.as_ref(),
                                    status_column(previous_status),
                                ],
                            )?;
                            if updated == 1 {
                                ScheduledRunTransitionResolution::Updated(run)
                            } else {
                                let current = load_scheduled_run_by_id(&transaction, run_id)?
                                    .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
                                ScheduledRunTransitionResolution::InvalidState(current)
                            }
                        }
                    }
                };
                transaction.commit()?;
                Ok(resolution)
            })
            .map_err(repository_error_from_database)
    }
}

fn map_history_row(row: &Row<'_>) -> Result<ScheduledTaskHistoryEntry, crate::DatabaseError> {
    match row.get::<_, String>("outcome")?.as_str() {
        "accepted" => Ok(ScheduledTaskHistoryEntry::Run(map_scheduled_run_row(row)?)),
        "missed" => Ok(ScheduledTaskHistoryEntry::Missed(
            ScheduledMissedOccurrence {
                occurrence: ScheduledOccurrence {
                    task_id: ScheduledTaskId::new(row.get::<_, String>("occurrence_task_id")?),
                    scheduled_at: row.get("occurrence_scheduled_at")?,
                },
                reason: parse_missed_reason(&row.get::<_, String>("missed_reason")?)?,
                observed_at: row.get("observed_at")?,
            },
        )),
        outcome => Err(crate::DatabaseError::CorruptScheduledOccurrenceOutcome(
            outcome.to_string(),
        )),
    }
}

fn prune_oldest_history(
    transaction: &Transaction<'_>,
    task_id: &ScheduledTaskId,
) -> Result<(), crate::DatabaseError> {
    let retained_offset = MAX_SCHEDULED_TASK_HISTORY_ENTRIES;
    transaction.execute(
        "DELETE FROM scheduled_runs
         WHERE task_id = ?1 AND scheduled_at IN (
             SELECT scheduled_at
             FROM scheduled_occurrences
             WHERE task_id = ?1
             ORDER BY scheduled_at DESC
             LIMIT -1 OFFSET ?2
         ) AND status NOT IN ('accepted', 'running')",
        params![task_id.as_ref(), retained_offset],
    )?;
    transaction.execute(
        "DELETE FROM scheduled_occurrences
         WHERE task_id = ?1 AND scheduled_at IN (
             SELECT scheduled_at
             FROM scheduled_occurrences
             WHERE task_id = ?1
             ORDER BY scheduled_at DESC
             LIMIT -1 OFFSET ?2
         ) AND NOT EXISTS (
             SELECT 1 FROM scheduled_runs
             WHERE scheduled_runs.task_id = scheduled_occurrences.task_id
               AND scheduled_runs.scheduled_at = scheduled_occurrences.scheduled_at
               AND scheduled_runs.status IN ('accepted', 'running')
         )",
        params![task_id.as_ref(), retained_offset],
    )?;
    Ok(())
}

fn admission_status(
    transaction: &Transaction<'_>,
    task_id: &ScheduledTaskId,
) -> Result<ScheduledRunStatus, crate::DatabaseError> {
    let task_active_count: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM scheduled_runs
         WHERE task_id = ?1 AND status IN ('accepted', 'running')",
        [task_id.as_ref()],
        |row| row.get(0),
    )?;
    if task_active_count > 0 {
        return Ok(ScheduledRunStatus::SkippedBusy);
    }
    let global_active_count: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM scheduled_runs
         WHERE status IN ('accepted', 'running')",
        [],
        |row| row.get(0),
    )?;
    if global_active_count >= MAX_CONCURRENT_LOCAL_SCHEDULED_RUNS {
        return Ok(ScheduledRunStatus::SkippedCapacity);
    }
    Ok(ScheduledRunStatus::Accepted)
}

fn candidate_columns(
    candidate: &ScheduledOccurrenceCandidate,
) -> (&'static str, Option<&'static str>, i64) {
    match candidate {
        ScheduledOccurrenceCandidate::Run(run) => ("accepted", None, run.created_at),
        ScheduledOccurrenceCandidate::Missed(missed) => (
            "missed",
            Some(missed_reason_column(missed.reason)),
            missed.observed_at,
        ),
    }
}

fn insert_scheduled_run(
    transaction: &Transaction<'_>,
    run: &ScheduledRun,
) -> Result<(), crate::DatabaseError> {
    let revision = i64::try_from(run.snapshot.revision).map_err(|_| {
        crate::DatabaseError::CorruptScheduledTaskRevision(run.snapshot.revision.to_string())
    })?;
    let (approval_policy, approval_consented_at) = approval_columns(run.snapshot.approval.as_ref());
    transaction.execute(
        "INSERT INTO scheduled_runs (
            id, task_id, scheduled_at, task_revision, project_id, agent_ref,
            model, prompt, approval_policy, approval_consented_at, status,
            session_id, worktree_path, failure_reason, created_at, updated_at
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16
         )",
        params![
            run.id.as_ref(),
            run.occurrence.task_id.as_ref(),
            run.occurrence.scheduled_at,
            revision,
            run.snapshot.project_id.as_ref(),
            run.snapshot.agent_ref.as_str(),
            run.snapshot.model.as_deref(),
            &run.snapshot.prompt,
            approval_policy,
            approval_consented_at,
            status_column(run.status),
            run.session_id.as_ref().map(AsRef::<str>::as_ref),
            run.worktree_path.as_deref(),
            run.failure_reason.map(failure_reason_column),
            run.created_at,
            run.updated_at,
        ],
    )?;
    Ok(())
}

fn load_existing_resolution(
    transaction: &Transaction<'_>,
    occurrence: &ScheduledOccurrence,
) -> Result<ScheduledOccurrenceResolution, crate::DatabaseError> {
    let (outcome, missed_reason, observed_at): (String, Option<String>, i64) = transaction
        .query_row(
            "SELECT outcome, missed_reason, observed_at
             FROM scheduled_occurrences
             WHERE task_id = ?1 AND scheduled_at = ?2",
            params![occurrence.task_id.as_ref(), occurrence.scheduled_at],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
    match outcome.as_str() {
        "accepted" if missed_reason.is_none() => Ok(ScheduledOccurrenceResolution::ExistingRun(
            load_scheduled_run(transaction, occurrence)?,
        )),
        "missed" => Ok(ScheduledOccurrenceResolution::ExistingMissed(
            ScheduledMissedOccurrence {
                occurrence: occurrence.clone(),
                reason: parse_missed_reason(missed_reason.as_deref().ok_or_else(|| {
                    crate::DatabaseError::CorruptScheduledMissReason("missing".to_string())
                })?)?,
                observed_at,
            },
        )),
        _ => Err(crate::DatabaseError::CorruptScheduledOccurrenceOutcome(
            format!("outcome={outcome:?}, missed_reason={missed_reason:?}"),
        )),
    }
}

fn load_scheduled_run(
    transaction: &Transaction<'_>,
    occurrence: &ScheduledOccurrence,
) -> Result<ScheduledRun, crate::DatabaseError> {
    let mut statement = transaction.prepare(
        "SELECT id, task_id, scheduled_at, task_revision, project_id, agent_ref,
                model, prompt, approval_policy, approval_consented_at, status,
                session_id, worktree_path, failure_reason, created_at, updated_at
         FROM scheduled_runs
         WHERE task_id = ?1 AND scheduled_at = ?2",
    )?;
    let mut rows = statement.query(params![
        occurrence.task_id.as_ref(),
        occurrence.scheduled_at,
    ])?;
    let row = rows.next()?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
    map_scheduled_run_row(row)
}

fn load_scheduled_run_by_id(
    transaction: &Transaction<'_>,
    run_id: &ScheduledRunId,
) -> Result<Option<ScheduledRun>, crate::DatabaseError> {
    let mut statement = transaction.prepare(
        "SELECT id, task_id, scheduled_at, task_revision, project_id, agent_ref,
                model, prompt, approval_policy, approval_consented_at, status,
                session_id, worktree_path, failure_reason, created_at, updated_at
         FROM scheduled_runs
         WHERE id = ?1",
    )?;
    let mut rows = statement.query([run_id.as_ref()])?;
    rows.next()?.map(map_scheduled_run_row).transpose()
}

fn map_scheduled_run_row(row: &Row<'_>) -> Result<ScheduledRun, crate::DatabaseError> {
    let revision_value = row.get::<_, i64>("task_revision")?;
    let revision = u64::try_from(revision_value)
        .ok()
        .filter(|revision| *revision > 0)
        .ok_or_else(|| {
            crate::DatabaseError::CorruptScheduledTaskRevision(revision_value.to_string())
        })?;
    let approval = map_approval_columns(
        row.get("approval_policy")?,
        row.get("approval_consented_at")?,
    )?;
    Ok(ScheduledRun {
        id: ScheduledRunId::new(row.get::<_, String>("id")?),
        occurrence: ScheduledOccurrence {
            task_id: ScheduledTaskId::new(row.get::<_, String>("task_id")?),
            scheduled_at: row.get("scheduled_at")?,
        },
        snapshot: ScheduledRunSnapshot {
            revision,
            project_id: ProjectId::new(row.get::<_, String>("project_id")?),
            agent_ref: AgentRef::parse(row.get::<_, String>("agent_ref")?)?,
            model: row.get("model")?,
            prompt: row.get("prompt")?,
            approval,
        },
        status: parse_status(&row.get::<_, String>("status")?)?,
        session_id: row
            .get::<_, Option<String>>("session_id")?
            .map(SessionId::new),
        worktree_path: row.get("worktree_path")?,
        failure_reason: row
            .get::<_, Option<String>>("failure_reason")?
            .map(|value| parse_failure_reason(&value))
            .transpose()?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

fn status_column(status: ScheduledRunStatus) -> &'static str {
    match status {
        ScheduledRunStatus::Accepted => "accepted",
        ScheduledRunStatus::Running => "running",
        ScheduledRunStatus::Succeeded => "succeeded",
        ScheduledRunStatus::Failed => "failed",
        ScheduledRunStatus::Cancelled => "cancelled",
        ScheduledRunStatus::SkippedBusy => "skipped_busy",
        ScheduledRunStatus::SkippedCapacity => "skipped_capacity",
    }
}

fn parse_status(value: &str) -> Result<ScheduledRunStatus, crate::DatabaseError> {
    match value {
        "accepted" => Ok(ScheduledRunStatus::Accepted),
        "running" => Ok(ScheduledRunStatus::Running),
        "succeeded" => Ok(ScheduledRunStatus::Succeeded),
        "failed" => Ok(ScheduledRunStatus::Failed),
        "cancelled" => Ok(ScheduledRunStatus::Cancelled),
        "skipped_busy" => Ok(ScheduledRunStatus::SkippedBusy),
        "skipped_capacity" => Ok(ScheduledRunStatus::SkippedCapacity),
        _ => Err(crate::DatabaseError::CorruptScheduledRunStatus(
            value.to_string(),
        )),
    }
}

fn failure_reason_column(reason: ScheduledRunFailureReason) -> &'static str {
    match reason {
        ScheduledRunFailureReason::TargetUnavailable => "target_unavailable",
        ScheduledRunFailureReason::ModelUnavailable => "model_unavailable",
        ScheduledRunFailureReason::WorktreeFailed => "worktree_failed",
        ScheduledRunFailureReason::ApprovalPolicyInvalid => "approval_policy_invalid",
        ScheduledRunFailureReason::Timeout => "timeout",
        ScheduledRunFailureReason::ExecutionError => "execution_error",
    }
}

fn parse_failure_reason(value: &str) -> Result<ScheduledRunFailureReason, crate::DatabaseError> {
    match value {
        "target_unavailable" => Ok(ScheduledRunFailureReason::TargetUnavailable),
        "model_unavailable" => Ok(ScheduledRunFailureReason::ModelUnavailable),
        "worktree_failed" => Ok(ScheduledRunFailureReason::WorktreeFailed),
        "approval_policy_invalid" => Ok(ScheduledRunFailureReason::ApprovalPolicyInvalid),
        "timeout" => Ok(ScheduledRunFailureReason::Timeout),
        "execution_error" => Ok(ScheduledRunFailureReason::ExecutionError),
        _ => Err(crate::DatabaseError::CorruptScheduledRunFailureReason(
            value.to_string(),
        )),
    }
}

fn missed_reason_column(reason: ScheduledMissReason) -> &'static str {
    match reason {
        ScheduledMissReason::AppNotRunning => "app_not_running",
        ScheduledMissReason::LateTrigger => "late_trigger",
    }
}

fn parse_missed_reason(value: &str) -> Result<ScheduledMissReason, crate::DatabaseError> {
    match value {
        "app_not_running" => Ok(ScheduledMissReason::AppNotRunning),
        "late_trigger" => Ok(ScheduledMissReason::LateTrigger),
        _ => Err(crate::DatabaseError::CorruptScheduledMissReason(
            value.to_string(),
        )),
    }
}

fn approval_columns(approval: Option<&ScheduledTaskApproval>) -> (Option<String>, Option<i64>) {
    approval
        .map(|approval| (Some(approval.policy.clone()), Some(approval.consented_at)))
        .unwrap_or((None, None))
}

fn map_approval_columns(
    policy: Option<String>,
    consented_at: Option<i64>,
) -> Result<Option<ScheduledTaskApproval>, crate::DatabaseError> {
    match (policy, consented_at) {
        (None, None) => Ok(None),
        (Some(policy), Some(consented_at))
            if policy == ora_domain::FULL_AUTO_IN_WORKTREE_POLICY =>
        {
            Ok(Some(ScheduledTaskApproval::full_auto_in_worktree(
                consented_at,
            )))
        }
        (policy, consented_at) => Err(crate::DatabaseError::CorruptScheduledTaskApproval(format!(
            "policy={policy:?}, consented_at={consented_at:?}"
        ))),
    }
}

fn repository_error_from_database(error: crate::DatabaseError) -> RepositoryError {
    RepositoryError::new(error)
}

#[cfg(test)]
mod tests {
    use ora_application::{
        ProjectRepository, ScheduledOccurrenceCandidate, ScheduledOccurrenceRepository,
        ScheduledOccurrenceResolution, ScheduledRunLifecycleRepository,
        ScheduledRunTransitionResolution, ScheduledTaskHistoryRepository, ScheduledTaskRepository,
    };
    use ora_domain::{
        AgentRef, AuditFields, Project, ProjectId, ScheduledMissReason, ScheduledMissedOccurrence,
        ScheduledOccurrence, ScheduledRun, ScheduledRunFailureReason, ScheduledRunId,
        ScheduledRunStatus, ScheduledRunTransition, ScheduledTask, ScheduledTaskApproval,
        ScheduledTaskExecutionLocation, ScheduledTaskHistoryEntry, ScheduledTaskId,
        ScheduledTaskSchedule, SessionId, WorkspaceLocation,
    };
    use pretty_assertions::assert_eq;
    use tempfile::TempDir;

    use crate::{
        DatabaseBootstrapper, DatabaseLocation, RepositoryPool, SqliteProjectRepository,
        SqliteScheduledTaskRepository, default_migration_catalog, test_clock::TestClock,
    };

    use super::SqliteScheduledRunRepository;

    #[test]
    fn persists_forward_lifecycle_and_rejects_a_late_terminal_callback() {
        let (temp_dir, pool) = bootstrapped_pool();
        seed_definition(&pool, temp_dir.path());
        let repository = SqliteScheduledRunRepository::new(pool);
        let run = ScheduledRun::accepted(
            ScheduledRunId::new("run-lifecycle"),
            occurrence(90),
            &scheduled_task(),
            91,
        );
        repository
            .resolve_scheduled_occurrence(ScheduledOccurrenceCandidate::Run(run))
            .expect("admit run");

        let running = repository
            .transition_scheduled_run(
                &ScheduledRunId::new("run-lifecycle"),
                ScheduledRunTransition::Start {
                    session_id: SessionId::new("session-1"),
                    worktree_path: "worktrees/run-lifecycle".to_string(),
                },
                100,
            )
            .expect("start run");
        assert!(matches!(
            running,
            ScheduledRunTransitionResolution::Updated(ScheduledRun {
                status: ScheduledRunStatus::Running,
                session_id: Some(_),
                worktree_path: Some(_),
                failure_reason: None,
                updated_at: 100,
                ..
            })
        ));

        let failed = repository
            .transition_scheduled_run(
                &ScheduledRunId::new("run-lifecycle"),
                ScheduledRunTransition::Fail {
                    reason: ScheduledRunFailureReason::Timeout,
                },
                200,
            )
            .expect("fail run");
        assert!(matches!(
            failed,
            ScheduledRunTransitionResolution::Updated(ScheduledRun {
                status: ScheduledRunStatus::Failed,
                failure_reason: Some(ScheduledRunFailureReason::Timeout),
                updated_at: 200,
                ..
            })
        ));

        let late_success = repository
            .transition_scheduled_run(
                &ScheduledRunId::new("run-lifecycle"),
                ScheduledRunTransition::Succeed,
                201,
            )
            .expect("reject late success");
        assert!(matches!(
            late_success,
            ScheduledRunTransitionResolution::InvalidState(ScheduledRun {
                status: ScheduledRunStatus::Failed,
                failure_reason: Some(ScheduledRunFailureReason::Timeout),
                updated_at: 200,
                ..
            })
        ));
    }

    #[test]
    fn replay_returns_the_first_run_and_its_frozen_snapshot() {
        let (temp_dir, pool) = bootstrapped_pool();
        seed_definition(&pool, temp_dir.path());
        let repository = SqliteScheduledRunRepository::new(pool);
        let task = scheduled_task();
        let occurrence = occurrence(100);
        let first =
            ScheduledRun::accepted(ScheduledRunId::new("run-1"), occurrence.clone(), &task, 101);
        assert_eq!(
            repository
                .resolve_scheduled_occurrence(ScheduledOccurrenceCandidate::Run(first.clone()))
                .expect("admit first run"),
            ScheduledOccurrenceResolution::Admitted(first.clone())
        );

        let mut edited_task = task;
        edited_task.revision = 2;
        edited_task.prompt = "New prompt".to_string();
        let replay =
            ScheduledRun::accepted(ScheduledRunId::new("run-2"), occurrence, &edited_task, 102);
        assert_eq!(
            repository
                .resolve_scheduled_occurrence(ScheduledOccurrenceCandidate::Run(replay))
                .expect("replay occurrence"),
            ScheduledOccurrenceResolution::ExistingRun(first)
        );
    }

    #[test]
    fn missed_outcome_prevents_a_later_run_for_the_same_occurrence() {
        let (temp_dir, pool) = bootstrapped_pool();
        seed_definition(&pool, temp_dir.path());
        let repository = SqliteScheduledRunRepository::new(pool.clone());
        let occurrence = occurrence(200);
        let missed = ScheduledMissedOccurrence {
            occurrence: occurrence.clone(),
            reason: ScheduledMissReason::LateTrigger,
            observed_at: 800_001,
        };
        assert_eq!(
            repository
                .resolve_scheduled_occurrence(ScheduledOccurrenceCandidate::Missed(missed.clone(),))
                .expect("record missed occurrence"),
            ScheduledOccurrenceResolution::Missed(missed.clone())
        );

        let run = ScheduledRun::accepted(
            ScheduledRunId::new("run-after-missed"),
            occurrence,
            &scheduled_task(),
            800_002,
        );
        assert_eq!(
            repository
                .resolve_scheduled_occurrence(ScheduledOccurrenceCandidate::Run(run))
                .expect("replay missed occurrence"),
            ScheduledOccurrenceResolution::ExistingMissed(missed)
        );
        let run_count: i64 = pool
            .with_connection(|connection| {
                connection
                    .query_row("SELECT COUNT(*) FROM scheduled_runs", [], |row| row.get(0))
                    .map_err(Into::into)
            })
            .expect("count runs");
        assert_eq!(run_count, 0_i64);
    }

    #[test]
    fn active_run_causes_the_same_task_to_persist_skipped_busy() {
        let (temp_dir, pool) = bootstrapped_pool();
        seed_definition(&pool, temp_dir.path());
        let repository = SqliteScheduledRunRepository::new(pool);
        let first = ScheduledRun::accepted(
            ScheduledRunId::new("run-active"),
            occurrence(300),
            &scheduled_task(),
            301,
        );
        let first = repository
            .resolve_scheduled_occurrence(ScheduledOccurrenceCandidate::Run(first))
            .expect("admit active run");
        assert!(matches!(
            first,
            ScheduledOccurrenceResolution::Admitted(ScheduledRun {
                status: ora_domain::ScheduledRunStatus::Accepted,
                ..
            })
        ));

        let second = ScheduledRun::accepted(
            ScheduledRunId::new("run-busy"),
            occurrence(400),
            &scheduled_task(),
            401,
        );
        let second = repository
            .resolve_scheduled_occurrence(ScheduledOccurrenceCandidate::Run(second))
            .expect("record busy skip");
        assert!(matches!(
            second,
            ScheduledOccurrenceResolution::Admitted(ScheduledRun {
                status: ora_domain::ScheduledRunStatus::SkippedBusy,
                session_id: None,
                ..
            })
        ));
    }

    #[test]
    fn fifth_active_task_persists_skipped_capacity() {
        let (temp_dir, pool) = bootstrapped_pool();
        seed_project(&pool, temp_dir.path());
        let repository = SqliteScheduledRunRepository::new(pool.clone());
        for index in 1_i64..=5 {
            SqliteScheduledTaskRepository::new(pool.clone())
                .create_scheduled_task(scheduled_task_with_id(&format!("scheduled-task-{index}")))
                .expect("create Scheduled Task");
        }

        for index in 1_i64..=4 {
            let task = scheduled_task_with_id(&format!("scheduled-task-{index}"));
            let run = ScheduledRun::accepted(
                ScheduledRunId::new(format!("run-{index}")),
                ScheduledOccurrence {
                    task_id: task.id.clone(),
                    scheduled_at: 1_000 + index,
                },
                &task,
                2_000 + index,
            );
            let resolution = repository
                .resolve_scheduled_occurrence(ScheduledOccurrenceCandidate::Run(run))
                .expect("admit within capacity");
            assert!(matches!(
                resolution,
                ScheduledOccurrenceResolution::Admitted(ScheduledRun {
                    status: ora_domain::ScheduledRunStatus::Accepted,
                    ..
                })
            ));
        }

        let fifth_task = scheduled_task_with_id("scheduled-task-5");
        let fifth = ScheduledRun::accepted(
            ScheduledRunId::new("run-5"),
            ScheduledOccurrence {
                task_id: fifth_task.id.clone(),
                scheduled_at: 1_005,
            },
            &fifth_task,
            2_005,
        );
        assert!(matches!(
            repository
                .resolve_scheduled_occurrence(ScheduledOccurrenceCandidate::Run(fifth))
                .expect("record capacity skip"),
            ScheduledOccurrenceResolution::Admitted(ScheduledRun {
                status: ora_domain::ScheduledRunStatus::SkippedCapacity,
                session_id: None,
                ..
            })
        ));
    }

    #[test]
    fn one_hundred_and_first_history_entry_prunes_only_the_oldest_fact() {
        let (temp_dir, pool) = bootstrapped_pool();
        seed_definition(&pool, temp_dir.path());
        let repository = SqliteScheduledRunRepository::new(pool.clone());

        for scheduled_at in 1_i64..=101 {
            let candidate = if scheduled_at % 2 == 0 {
                ScheduledOccurrenceCandidate::Run(ScheduledRun::accepted(
                    ScheduledRunId::new(format!("run-{scheduled_at}")),
                    occurrence(scheduled_at),
                    &scheduled_task(),
                    scheduled_at,
                ))
            } else {
                ScheduledOccurrenceCandidate::Missed(ScheduledMissedOccurrence {
                    occurrence: occurrence(scheduled_at),
                    reason: ScheduledMissReason::AppNotRunning,
                    observed_at: scheduled_at,
                })
            };
            repository
                .resolve_scheduled_occurrence(candidate)
                .expect("persist history fact");
        }

        let (history_count, oldest, newest, run_count): (i64, i64, i64, i64) = pool
            .with_connection(|connection| {
                let (history_count, oldest, newest) = connection.query_row(
                    "SELECT COUNT(*), MIN(scheduled_at), MAX(scheduled_at)
                     FROM scheduled_occurrences
                     WHERE task_id = 'scheduled-task-1'",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )?;
                let run_count = connection.query_row(
                    "SELECT COUNT(*) FROM scheduled_runs
                     WHERE task_id = 'scheduled-task-1'",
                    [],
                    |row| row.get(0),
                )?;
                Ok((history_count, oldest, newest, run_count))
            })
            .expect("read retained history");

        assert_eq!(history_count, 100);
        assert_eq!(oldest, 2);
        assert_eq!(newest, 101);
        assert_eq!(run_count, 50);
    }

    #[test]
    fn retention_never_prunes_an_active_run_needed_for_admission() {
        let (temp_dir, pool) = bootstrapped_pool();
        seed_definition(&pool, temp_dir.path());
        let repository = SqliteScheduledRunRepository::new(pool.clone());
        repository
            .resolve_scheduled_occurrence(ScheduledOccurrenceCandidate::Run(
                ScheduledRun::accepted(
                    ScheduledRunId::new("run-active"),
                    occurrence(1),
                    &scheduled_task(),
                    1,
                ),
            ))
            .expect("persist active run");
        for scheduled_at in 2_i64..=101 {
            repository
                .resolve_scheduled_occurrence(ScheduledOccurrenceCandidate::Missed(
                    ScheduledMissedOccurrence {
                        occurrence: occurrence(scheduled_at),
                        reason: ScheduledMissReason::AppNotRunning,
                        observed_at: scheduled_at,
                    },
                ))
                .expect("persist missed fact");
        }

        let (active_count, occurrence_count): (i64, i64) = pool
            .with_connection(|connection| {
                let active_count = connection.query_row(
                    "SELECT COUNT(*) FROM scheduled_runs
                     WHERE id = 'run-active' AND status = 'accepted'",
                    [],
                    |row| row.get(0),
                )?;
                let occurrence_count = connection.query_row(
                    "SELECT COUNT(*) FROM scheduled_occurrences
                     WHERE task_id = 'scheduled-task-1'",
                    [],
                    |row| row.get(0),
                )?;
                Ok((active_count, occurrence_count))
            })
            .expect("read active retention state");

        assert_eq!(active_count, 1);
        assert_eq!(occurrence_count, 101);
    }

    #[test]
    fn history_reads_newest_first_after_definition_delete_and_repository_restart() {
        let (temp_dir, pool) = bootstrapped_pool();
        seed_definition(&pool, temp_dir.path());
        let repository = SqliteScheduledRunRepository::new(pool.clone());
        let run = ScheduledRun::accepted(
            ScheduledRunId::new("run-older"),
            occurrence(10),
            &scheduled_task(),
            11,
        );
        repository
            .resolve_scheduled_occurrence(ScheduledOccurrenceCandidate::Run(run.clone()))
            .expect("persist run");
        let missed = ScheduledMissedOccurrence {
            occurrence: occurrence(20),
            reason: ScheduledMissReason::LateTrigger,
            observed_at: 620_001,
        };
        repository
            .resolve_scheduled_occurrence(ScheduledOccurrenceCandidate::Missed(missed.clone()))
            .expect("persist missed fact");
        SqliteScheduledTaskRepository::new(pool.clone())
            .soft_delete_scheduled_task(&ScheduledTaskId::new("scheduled-task-1"), 30)
            .expect("soft-delete definition");

        let restarted_repository = SqliteScheduledRunRepository::new(pool);
        assert_eq!(
            restarted_repository
                .list_scheduled_task_history(&ScheduledTaskId::new("scheduled-task-1"))
                .expect("list retained history"),
            vec![
                ScheduledTaskHistoryEntry::Missed(missed),
                ScheduledTaskHistoryEntry::Run(run),
            ]
        );
    }

    fn bootstrapped_pool() -> (TempDir, RepositoryPool) {
        let temp_dir = TempDir::new().expect("create temp directory");
        let database_path = temp_dir.path().join("scheduled-run.sqlite3");
        let pool = DatabaseBootstrapper::new(TestClock::new(1))
            .bootstrap_repository_pool(
                &DatabaseLocation::path(database_path),
                &default_migration_catalog().expect("build migration catalog"),
            )
            .expect("bootstrap database");
        (temp_dir, pool)
    }

    fn seed_definition(pool: &RepositoryPool, temp_root: &std::path::Path) {
        seed_project(pool, temp_root);
        SqliteScheduledTaskRepository::new(pool.clone())
            .create_scheduled_task(scheduled_task())
            .expect("create Scheduled Task");
    }

    fn seed_project(pool: &RepositoryPool, temp_root: &std::path::Path) {
        let workspace_root = temp_root.join("project");
        std::fs::create_dir_all(&workspace_root).expect("create project workspace");
        SqliteProjectRepository::with_clock(pool.clone(), TestClock::new(1))
            .create_project(
                Project::new(
                    ProjectId::new("project-1"),
                    "Project",
                    AuditFields::new(1, 1, false),
                ),
                WorkspaceLocation::local_filesystem(workspace_root.to_string_lossy()),
            )
            .expect("create project");
    }

    fn occurrence(scheduled_at: i64) -> ScheduledOccurrence {
        ScheduledOccurrence {
            task_id: ScheduledTaskId::new("scheduled-task-1"),
            scheduled_at,
        }
    }

    fn scheduled_task() -> ScheduledTask {
        scheduled_task_with_id("scheduled-task-1")
    }

    fn scheduled_task_with_id(task_id: &str) -> ScheduledTask {
        ScheduledTask::new(
            ScheduledTaskId::new(task_id),
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
}
