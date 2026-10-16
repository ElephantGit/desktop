use super::Migration;

const UP_STATEMENTS: &[&str] = &[r#"
ALTER TABLE scheduled_runs ADD COLUMN worktree_path TEXT NULL;
ALTER TABLE scheduled_runs ADD COLUMN failure_reason TEXT NULL CHECK (
    (status = 'failed' AND failure_reason IN (
            'target_unavailable', 'model_unavailable', 'worktree_failed',
            'approval_policy_invalid', 'timeout', 'execution_error'
        ))
    OR (status <> 'failed' AND failure_reason IS NULL)
);
"#];

const DOWN_STATEMENTS: &[&str] = &[r#"
ALTER TABLE scheduled_runs DROP COLUMN failure_reason;
ALTER TABLE scheduled_runs DROP COLUMN worktree_path;
"#];

/// Adds durable execution references and the bounded Scheduled Run failure vocabulary.
pub fn migration() -> Migration {
    Migration::new("0018", UP_STATEMENTS, DOWN_STATEMENTS)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use rusqlite::Connection;

    use super::{DOWN_STATEMENTS, UP_STATEMENTS};

    #[test]
    fn stores_only_standard_failure_reasons_and_rolls_back() {
        let connection = Connection::open_in_memory().expect("open database");
        connection
            .execute_batch(
                "CREATE TABLE scheduled_runs (
                    id TEXT PRIMARY KEY NOT NULL,
                    status TEXT NOT NULL
                 );\n\
                 INSERT INTO scheduled_runs (id, status) VALUES ('run-1', 'accepted');",
            )
            .expect("create prior schema fixture");
        for statement in UP_STATEMENTS {
            connection
                .execute_batch(statement)
                .expect("apply Scheduled Run lifecycle migration");
        }

        connection
            .execute(
                "UPDATE scheduled_runs
                 SET status = 'failed', worktree_path = 'worktrees/run-1',
                     failure_reason = 'timeout'
                 WHERE id = 'run-1'",
                [],
            )
            .expect("store lifecycle details");
        let values: (String, String) = connection
            .query_row(
                "SELECT worktree_path, failure_reason FROM scheduled_runs WHERE id = 'run-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("read lifecycle details");
        assert_eq!(
            values,
            ("worktrees/run-1".to_string(), "timeout".to_string())
        );
        assert!(
            connection
                .execute(
                    "UPDATE scheduled_runs SET failure_reason = 'unknown' WHERE id = 'run-1'",
                    [],
                )
                .is_err()
        );
        connection
            .execute(
                "UPDATE scheduled_runs SET status = 'accepted', failure_reason = NULL
                 WHERE id = 'run-1'",
                [],
            )
            .expect("restore non-failed state");
        assert!(
            connection
                .execute(
                    "UPDATE scheduled_runs SET failure_reason = 'timeout' WHERE id = 'run-1'",
                    [],
                )
                .is_err()
        );

        for statement in DOWN_STATEMENTS {
            connection
                .execute_batch(statement)
                .expect("roll back Scheduled Run lifecycle migration");
        }
        let lifecycle_column_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('scheduled_runs')
                 WHERE name IN ('worktree_path', 'failure_reason')",
                [],
                |row| row.get(0),
            )
            .expect("count lifecycle columns");
        assert_eq!(lifecycle_column_count, 0);
    }
}
