use ora_application::{
    RepositoryError, ScheduledOccurrenceCandidate, ScheduledOccurrenceRepository,
    ScheduledOccurrenceResolution,
};
use ora_domain::{
    AgentRef, ProjectId, ScheduledMissReason, ScheduledMissedOccurrence, ScheduledOccurrence,
    ScheduledRun, ScheduledRunId, ScheduledRunSnapshot, ScheduledRunStatus, ScheduledTaskApproval,
    ScheduledTaskId, SessionId,
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
                            insert_scheduled_run(&transaction, run)?;
                            ScheduledOccurrenceResolution::Admitted(run.clone())
                        }
                        ScheduledOccurrenceCandidate::Missed(missed) => {
                            ScheduledOccurrenceResolution::Missed(missed.clone())
                        }
                    }
                } else {
                    load_existing_resolution(&transaction, &occurrence)?
                };
                transaction.commit()?;
                Ok(resolution)
            })
            .map_err(repository_error_from_database)
    }
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
            session_id, created_at, updated_at
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14
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
                session_id, created_at, updated_at
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
        ScheduledOccurrenceResolution, ScheduledTaskRepository,
    };
    use ora_domain::{
        AgentRef, AuditFields, Project, ProjectId, ScheduledMissReason, ScheduledMissedOccurrence,
        ScheduledOccurrence, ScheduledRun, ScheduledRunId, ScheduledTask, ScheduledTaskApproval,
        ScheduledTaskExecutionLocation, ScheduledTaskId, ScheduledTaskSchedule, WorkspaceLocation,
    };
    use pretty_assertions::assert_eq;
    use tempfile::TempDir;

    use crate::{
        DatabaseBootstrapper, DatabaseLocation, RepositoryPool, SqliteProjectRepository,
        SqliteScheduledTaskRepository, default_migration_catalog, test_clock::TestClock,
    };

    use super::SqliteScheduledRunRepository;

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
        SqliteScheduledTaskRepository::new(pool.clone())
            .create_scheduled_task(scheduled_task())
            .expect("create Scheduled Task");
    }

    fn occurrence(scheduled_at: i64) -> ScheduledOccurrence {
        ScheduledOccurrence {
            task_id: ScheduledTaskId::new("scheduled-task-1"),
            scheduled_at,
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
}
