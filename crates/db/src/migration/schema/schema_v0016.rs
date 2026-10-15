use super::Migration;

const UP_STATEMENTS: &[&str] = &[r#"
CREATE TABLE scheduled_runs (
    id                    TEXT PRIMARY KEY NOT NULL,
    task_id               TEXT NOT NULL REFERENCES scheduled_tasks(id),
    scheduled_at          INTEGER NOT NULL,
    task_revision         INTEGER NOT NULL CHECK (task_revision >= 1),
    project_id            TEXT NOT NULL,
    agent_ref             TEXT NOT NULL,
    model                 TEXT NULL,
    prompt                TEXT NOT NULL,
    approval_policy       TEXT NULL CHECK (
        approval_policy IS NULL OR approval_policy = 'full_auto_in_worktree'
    ),
    approval_consented_at INTEGER NULL,
    status                TEXT NOT NULL CHECK (status IN (
        'accepted', 'running', 'succeeded', 'failed', 'cancelled',
        'skipped_busy', 'skipped_capacity'
    )),
    session_id            TEXT NULL,
    created_at            INTEGER NOT NULL,
    updated_at            INTEGER NOT NULL,
    UNIQUE (task_id, scheduled_at),
    CHECK (
        (approval_policy IS NULL AND approval_consented_at IS NULL)
        OR (approval_policy IS NOT NULL AND approval_consented_at IS NOT NULL)
    )
);

CREATE INDEX idx_scheduled_runs_task_history
    ON scheduled_runs(task_id, scheduled_at DESC, id);
"#];

const DOWN_STATEMENTS: &[&str] = &[r#"
DROP INDEX IF EXISTS idx_scheduled_runs_task_history;
DROP TABLE IF EXISTS scheduled_runs;
"#];

/// Persists one immutable definition snapshot for each admitted planned occurrence.
pub fn migration() -> Migration {
    Migration::new("0016", UP_STATEMENTS, DOWN_STATEMENTS)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use rusqlite::Connection;

    use super::{DOWN_STATEMENTS, UP_STATEMENTS};

    #[test]
    fn enforces_one_run_per_task_and_scheduled_time() {
        let connection = Connection::open_in_memory().expect("open database");
        connection
            .execute_batch(
                "CREATE TABLE scheduled_tasks (id TEXT PRIMARY KEY NOT NULL);\n\
                 INSERT INTO scheduled_tasks (id) VALUES ('task-1');",
            )
            .expect("create task fixture");
        for statement in UP_STATEMENTS {
            connection
                .execute_batch(statement)
                .expect("apply Scheduled Run migration");
        }

        let insert = |id: &str| {
            connection.execute(
                "INSERT INTO scheduled_runs (
                    id, task_id, scheduled_at, task_revision, project_id, agent_ref,
                    prompt, approval_policy, approval_consented_at, status, created_at, updated_at
                 ) VALUES (?1, 'task-1', 100, 1, 'project-1', 'ora/agent', 'Prompt',
                    'full_auto_in_worktree', 10, 'accepted', 10, 10)",
                [id],
            )
        };
        assert_eq!(insert("run-1").expect("insert first run"), 1);
        assert!(insert("run-2").is_err());

        for statement in DOWN_STATEMENTS {
            connection
                .execute_batch(statement)
                .expect("roll back Scheduled Run migration");
        }
    }
}
