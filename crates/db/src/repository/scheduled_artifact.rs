use ora_application::{
    RepositoryError, ScheduledArtifactRunRecord, ScheduledRunArtifactRepository,
};
use ora_domain::{
    ScheduledArtifactKind, ScheduledArtifactRejection, ScheduledArtifactRejectionReason,
    ScheduledRunArtifact, ScheduledRunId, ScheduledTaskId,
};
use rusqlite::{Row, params};

use crate::repository::scheduled_run::SqliteScheduledRunRepository;

/// Persists one run's artifact manifest and the candidates collection refused.
///
/// The table has no foreign key to `scheduled_runs` because history retention deletes run
/// rows before the filesystem cleanup runs; the sweep then recognises those orphan rows and
/// removes both the rows and their stored files.
impl ScheduledRunArtifactRepository for SqliteScheduledRunRepository {
    fn record_scheduled_run_artifacts(
        &self,
        run_id: &ScheduledRunId,
        artifacts: &[ScheduledRunArtifact],
        rejections: &[ScheduledArtifactRejection],
        recorded_at: i64,
    ) -> Result<(), RepositoryError> {
        self.pool()
            .with_connection_mut(|connection| {
                let transaction = connection.transaction()?;
                for artifact in artifacts {
                    let size_bytes = i64::try_from(artifact.size_bytes).map_err(|_| {
                        crate::DatabaseError::CorruptScheduledArtifactSize(
                            artifact.relative_path.clone(),
                        )
                    })?;
                    transaction.execute(
                        "INSERT INTO scheduled_run_artifacts
                             (run_id, relative_path, kind, size_bytes, registered_at)
                         VALUES (?1, ?2, ?3, ?4, ?5)",
                        params![
                            run_id.as_ref(),
                            artifact.relative_path.as_str(),
                            artifact.kind.as_str(),
                            size_bytes,
                            artifact.registered_at,
                        ],
                    )?;
                }
                for rejection in rejections {
                    transaction.execute(
                        "INSERT INTO scheduled_run_artifact_rejections
                             (run_id, name, reason, registered_at)
                         VALUES (?1, ?2, ?3, ?4)",
                        params![
                            run_id.as_ref(),
                            rejection.name.as_str(),
                            rejection.reason.as_str(),
                            recorded_at,
                        ],
                    )?;
                }
                transaction.commit()?;
                Ok(())
            })
            .map_err(repository_error_from_database)
    }

    fn list_scheduled_run_artifacts(
        &self,
        run_id: &ScheduledRunId,
    ) -> Result<Vec<ScheduledRunArtifact>, RepositoryError> {
        self.pool()
            .with_connection(|connection| {
                let mut statement = connection.prepare(
                    "SELECT relative_path, kind, size_bytes, registered_at
                     FROM scheduled_run_artifacts
                     WHERE run_id = ?1
                     ORDER BY registered_at, relative_path",
                )?;
                let mut rows = statement.query([run_id.as_ref()])?;
                let mut artifacts = Vec::new();
                while let Some(row) = rows.next()? {
                    artifacts.push(map_artifact_row(row)?);
                }
                Ok(artifacts)
            })
            .map_err(repository_error_from_database)
    }

    fn list_scheduled_run_artifact_rejections(
        &self,
        run_id: &ScheduledRunId,
    ) -> Result<Vec<ScheduledArtifactRejection>, RepositoryError> {
        self.pool()
            .with_connection(|connection| {
                let mut statement = connection.prepare(
                    "SELECT name, reason FROM scheduled_run_artifact_rejections
                     WHERE run_id = ?1
                     ORDER BY name",
                )?;
                let mut rows = statement.query([run_id.as_ref()])?;
                let mut rejections = Vec::new();
                while let Some(row) = rows.next()? {
                    rejections.push(map_rejection_row(row)?);
                }
                Ok(rejections)
            })
            .map_err(repository_error_from_database)
    }

    fn find_scheduled_run_artifact(
        &self,
        run_id: &ScheduledRunId,
        relative_path: &str,
    ) -> Result<Option<ScheduledRunArtifact>, RepositoryError> {
        self.pool()
            .with_connection(|connection| {
                let mut statement = connection.prepare(
                    "SELECT relative_path, kind, size_bytes, registered_at
                     FROM scheduled_run_artifacts
                     WHERE run_id = ?1 AND relative_path = ?2",
                )?;
                let mut rows = statement.query(params![run_id.as_ref(), relative_path])?;
                rows.next()?.map(map_artifact_row).transpose()
            })
            .map_err(repository_error_from_database)
    }

    fn list_scheduled_artifact_runs(
        &self,
    ) -> Result<Vec<ScheduledArtifactRunRecord>, RepositoryError> {
        self.pool()
            .with_connection(|connection| {
                let mut statement = connection.prepare(
                    "SELECT owned.run_id AS run_id,
                            r.task_id AS task_id,
                            r.updated_at AS updated_at,
                            r.worktree_task_id AS worktree_task_id,
                            CASE WHEN r.id IS NULL THEN 1 ELSE 0 END AS run_missing,
                            CASE WHEN t.id IS NULL OR t.is_deleted != 0 THEN 1 ELSE 0 END
                                AS task_deleted
                     FROM (
                         SELECT run_id FROM scheduled_run_artifacts
                         UNION
                         SELECT run_id FROM scheduled_run_artifact_rejections
                     ) AS owned
                     LEFT JOIN scheduled_runs r ON r.id = owned.run_id
                     LEFT JOIN scheduled_tasks t ON t.id = r.task_id
                     ORDER BY r.updated_at ASC, owned.run_id ASC",
                )?;
                let mut rows = statement.query([])?;
                let mut records = Vec::new();
                while let Some(row) = rows.next()? {
                    records.push(map_artifact_run_row(row)?);
                }
                Ok(records)
            })
            .map_err(repository_error_from_database)
    }

    fn find_scheduled_artifact_run(
        &self,
        run_id: &ScheduledRunId,
    ) -> Result<Option<ScheduledArtifactRunRecord>, RepositoryError> {
        self.pool()
            .with_connection(|connection| {
                let mut statement = connection.prepare(
                    "SELECT requested.run_id AS run_id,
                            r.task_id AS task_id,
                            r.updated_at AS updated_at,
                            r.worktree_task_id AS worktree_task_id,
                            CASE WHEN r.id IS NULL THEN 1 ELSE 0 END AS run_missing,
                            CASE WHEN t.id IS NULL OR t.is_deleted != 0 THEN 1 ELSE 0 END
                                AS task_deleted
                     FROM (SELECT ?1 AS run_id) AS requested
                     LEFT JOIN scheduled_runs r ON r.id = requested.run_id
                     LEFT JOIN scheduled_tasks t ON t.id = r.task_id
                     WHERE r.id IS NOT NULL
                        OR EXISTS (
                            SELECT 1 FROM scheduled_run_artifacts a
                            WHERE a.run_id = requested.run_id
                        )
                        OR EXISTS (
                            SELECT 1 FROM scheduled_run_artifact_rejections j
                            WHERE j.run_id = requested.run_id
                        )",
                )?;
                let mut rows = statement.query([run_id.as_ref()])?;
                rows.next()?.map(map_artifact_run_row).transpose()
            })
            .map_err(repository_error_from_database)
    }

    fn delete_scheduled_run_artifact(
        &self,
        run_id: &ScheduledRunId,
        relative_path: &str,
    ) -> Result<bool, RepositoryError> {
        self.pool()
            .with_connection_mut(|connection| {
                Ok(connection.execute(
                    "DELETE FROM scheduled_run_artifacts
                     WHERE run_id = ?1 AND relative_path = ?2",
                    params![run_id.as_ref(), relative_path],
                )? > 0)
            })
            .map_err(repository_error_from_database)
    }

    fn delete_scheduled_run_artifact_rows(
        &self,
        run_id: &ScheduledRunId,
    ) -> Result<usize, RepositoryError> {
        self.pool()
            .with_connection_mut(|connection| {
                let transaction = connection.transaction()?;
                let artifacts = transaction.execute(
                    "DELETE FROM scheduled_run_artifacts WHERE run_id = ?1",
                    [run_id.as_ref()],
                )?;
                let rejections = transaction.execute(
                    "DELETE FROM scheduled_run_artifact_rejections WHERE run_id = ?1",
                    [run_id.as_ref()],
                )?;
                transaction.commit()?;
                Ok(artifacts + rejections)
            })
            .map_err(repository_error_from_database)
    }
}

fn map_artifact_row(row: &Row<'_>) -> Result<ScheduledRunArtifact, crate::DatabaseError> {
    let kind_value: String = row.get("kind")?;
    let kind = ScheduledArtifactKind::parse(&kind_value).ok_or(
        crate::DatabaseError::CorruptScheduledArtifactKind(kind_value),
    )?;
    let size_value: i64 = row.get("size_bytes")?;
    let size_bytes = u64::try_from(size_value)
        .map_err(|_| crate::DatabaseError::CorruptScheduledArtifactSize(size_value.to_string()))?;
    Ok(ScheduledRunArtifact {
        relative_path: row.get("relative_path")?,
        kind,
        size_bytes,
        registered_at: row.get("registered_at")?,
    })
}

fn map_rejection_row(row: &Row<'_>) -> Result<ScheduledArtifactRejection, crate::DatabaseError> {
    let reason_value: String = row.get("reason")?;
    let reason = ScheduledArtifactRejectionReason::parse(&reason_value)
        .ok_or(crate::DatabaseError::CorruptScheduledArtifactRejectionReason(reason_value))?;
    Ok(ScheduledArtifactRejection {
        name: row.get("name")?,
        reason,
    })
}

fn map_artifact_run_row(row: &Row<'_>) -> Result<ScheduledArtifactRunRecord, crate::DatabaseError> {
    Ok(ScheduledArtifactRunRecord {
        run_id: ScheduledRunId::new(row.get::<_, String>("run_id")?),
        task_id: row
            .get::<_, Option<String>>("task_id")?
            .map(ScheduledTaskId::new),
        updated_at: row.get("updated_at")?,
        run_missing: row.get::<_, i64>("run_missing")? != 0,
        task_deleted: row.get::<_, i64>("task_deleted")? != 0,
        retains_worktree: row.get::<_, Option<String>>("worktree_task_id")?.is_some(),
    })
}

fn repository_error_from_database(error: crate::DatabaseError) -> RepositoryError {
    RepositoryError::new(error)
}

#[cfg(test)]
mod tests;
