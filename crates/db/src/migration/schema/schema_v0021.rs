use super::Migration;

const UP_STATEMENTS: &[&str] = &[
    "CREATE TABLE scheduled_run_artifacts (
        run_id TEXT NOT NULL,
        relative_path TEXT NOT NULL,
        kind TEXT NOT NULL CHECK (
            kind IN ('markdown', 'json', 'text', 'log', 'xml', 'html')
        ),
        size_bytes INTEGER NOT NULL CHECK (size_bytes >= 0),
        registered_at INTEGER NOT NULL,
        PRIMARY KEY (run_id, relative_path)
    );",
    "CREATE TABLE scheduled_run_artifact_rejections (
        run_id TEXT NOT NULL,
        name TEXT NOT NULL,
        reason TEXT NOT NULL CHECK (
            reason IN (
                'unsupported_type', 'path_unsafe', 'symlink', 'hard_link',
                'too_large', 'too_many', 'total_too_large', 'read_failed',
                'copy_failed'
            )
        ),
        registered_at INTEGER NOT NULL,
        PRIMARY KEY (run_id, name)
    );",
];

const DOWN_STATEMENTS: &[&str] = &[
    "DROP TABLE IF EXISTS scheduled_run_artifact_rejections;",
    "DROP TABLE IF EXISTS scheduled_run_artifacts;",
];

/// Adds the per-run artifact manifest and rejected-candidate log so a Scheduled Run's
/// collected artifacts outlive its provisioning worktree and remain readable from history.
pub fn migration() -> Migration {
    Migration::new("0021", UP_STATEMENTS, DOWN_STATEMENTS)
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::UP_STATEMENTS;

    #[test]
    fn registers_artifacts_and_rejections_under_a_run_id() {
        let connection = Connection::open_in_memory().expect("open database");
        for statement in UP_STATEMENTS {
            connection
                .execute_batch(statement)
                .expect("apply migration");
        }
        connection
            .execute(
                "INSERT INTO scheduled_run_artifacts (run_id, relative_path, kind, size_bytes, registered_at)
                 VALUES ('run-1', 'report.md', 'markdown', 100, 1)",
                [],
            )
            .expect("insert artifact");
        connection
            .execute(
                "INSERT INTO scheduled_run_artifact_rejections (run_id, name, reason, registered_at)
                 VALUES ('run-1', 'secret.pdf', 'unsupported_type', 1)",
                [],
            )
            .expect("insert rejection");

        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM scheduled_run_artifacts WHERE run_id = 'run-1'",
                [],
                |row| row.get(0),
            )
            .expect("count artifacts");
        assert_eq!(count, 1);
        let rejections: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM scheduled_run_artifact_rejections WHERE run_id = 'run-1'",
                [],
                |row| row.get(0),
            )
            .expect("count rejections");
        assert_eq!(rejections, 1);
    }

    #[test]
    fn rejects_artifact_kinds_outside_the_closed_whitelist() {
        let connection = Connection::open_in_memory().expect("open database");
        for statement in UP_STATEMENTS {
            connection
                .execute_batch(statement)
                .expect("apply migration");
        }
        assert!(
            connection
                .execute(
                    "INSERT INTO scheduled_run_artifacts
                         (run_id, relative_path, kind, size_bytes, registered_at)
                     VALUES ('run-1', 'payload.exe', 'executable', 1, 1)",
                    [],
                )
                .is_err()
        );
    }
}
