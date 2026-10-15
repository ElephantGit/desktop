use ora_application::{RepositoryError, ScheduledRunAdmission, ScheduledRunRepository};
use ora_domain::{
    AgentRef, ProjectId, ScheduledOccurrence, ScheduledRun, ScheduledRunId, ScheduledRunSnapshot,
    ScheduledRunStatus, ScheduledTaskApproval, ScheduledTaskId, SessionId,
};
use rusqlite::{Row, params};

use crate::repository::RepositoryPool;

/// Atomically admits planned Scheduled Task occurrences through the shared SQLite pool.
#[derive(Clone, Debug)]
pub struct SqliteScheduledRunRepository {
    pool: RepositoryPool,
}

impl SqliteScheduledRunRepository {
    /// Builds a Scheduled Run repository from the shared SQLite connection pool.
    pub fn new(pool: RepositoryPool) -> Self {
        Self { pool }
    }
}

impl ScheduledRunRepository for SqliteScheduledRunRepository {
    fn admit_scheduled_run(
        &self,
        candidate: ScheduledRun,
    ) -> Result<ScheduledRunAdmission, RepositoryError> {
        let revision = serialize_revision(candidate.snapshot.revision)?;
        let (approval_policy, approval_consented_at) =
            approval_columns(candidate.snapshot.approval.as_ref());
        self.pool
            .with_connection_mut(|connection| {
                let transaction = connection.transaction()?;
                let inserted = transaction.execute(
                    "INSERT INTO scheduled_runs (
                        id, task_id, scheduled_at, task_revision, project_id, agent_ref,
                        model, prompt, approval_policy, approval_consented_at, status,
                        session_id, created_at, updated_at
                     ) VALUES (
                        ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14
                     )
                     ON CONFLICT(task_id, scheduled_at) DO NOTHING",
                    params![
                        candidate.id.as_ref(),
                        candidate.occurrence.task_id.as_ref(),
                        candidate.occurrence.scheduled_at,
                        revision,
                        candidate.snapshot.project_id.as_ref(),
                        candidate.snapshot.agent_ref.as_str(),
                        candidate.snapshot.model.as_deref(),
                        &candidate.snapshot.prompt,
                        approval_policy,
                        approval_consented_at,
                        status_column(candidate.status),
                        candidate.session_id.as_ref().map(AsRef::<str>::as_ref),
                        candidate.created_at,
                        candidate.updated_at,
                    ],
                )? > 0;
                let persisted = {
                    let mut statement = transaction.prepare(
                        "SELECT id, task_id, scheduled_at, task_revision, project_id, agent_ref,
                                model, prompt, approval_policy, approval_consented_at, status,
                                session_id, created_at, updated_at
                         FROM scheduled_runs
                         WHERE task_id = ?1 AND scheduled_at = ?2",
                    )?;
                    let mut rows = statement.query(params![
                        candidate.occurrence.task_id.as_ref(),
                        candidate.occurrence.scheduled_at,
                    ])?;
                    let row = rows.next()?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
                    map_scheduled_run_row(row)?
                };
                transaction.commit()?;
                Ok(if inserted {
                    ScheduledRunAdmission::Admitted(persisted)
                } else {
                    ScheduledRunAdmission::Existing(persisted)
                })
            })
            .map_err(repository_error_from_database)
    }

    fn find_scheduled_run(
        &self,
        occurrence: &ScheduledOccurrence,
    ) -> Result<Option<ScheduledRun>, RepositoryError> {
        self.pool
            .with_connection(|connection| {
                let mut statement = connection.prepare(
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
                rows.next()?.map(map_scheduled_run_row).transpose()
            })
            .map_err(repository_error_from_database)
    }
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

fn serialize_revision(revision: u64) -> Result<i64, RepositoryError> {
    i64::try_from(revision).map_err(|_| {
        repository_error_from_database(crate::DatabaseError::CorruptScheduledTaskRevision(
            revision.to_string(),
        ))
    })
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
        ProjectRepository, ScheduledRunAdmission, ScheduledRunRepository, ScheduledTaskRepository,
    };
    use ora_domain::{
        AgentRef, AuditFields, Project, ProjectId, ScheduledOccurrence, ScheduledRun,
        ScheduledRunId, ScheduledTask, ScheduledTaskApproval, ScheduledTaskExecutionLocation,
        ScheduledTaskId, ScheduledTaskSchedule, WorkspaceLocation,
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
        let occurrence = ScheduledOccurrence {
            task_id: task.id.clone(),
            scheduled_at: 100,
        };
        let first =
            ScheduledRun::accepted(ScheduledRunId::new("run-1"), occurrence.clone(), &task, 101);
        assert_eq!(
            repository
                .admit_scheduled_run(first.clone())
                .expect("admit first run"),
            ScheduledRunAdmission::Admitted(first.clone())
        );

        let mut edited_task = task;
        edited_task.revision = 2;
        edited_task.prompt = "New prompt".to_string();
        let replay = ScheduledRun::accepted(
            ScheduledRunId::new("run-2"),
            occurrence.clone(),
            &edited_task,
            102,
        );
        assert_eq!(
            repository
                .admit_scheduled_run(replay)
                .expect("replay occurrence"),
            ScheduledRunAdmission::Existing(first.clone())
        );
        assert_eq!(
            repository
                .find_scheduled_run(&occurrence)
                .expect("find admitted run"),
            Some(first)
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
