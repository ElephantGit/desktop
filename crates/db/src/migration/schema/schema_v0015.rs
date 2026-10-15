use super::Migration;

const UP_STATEMENTS: &[&str] = &[r#"
CREATE TABLE scheduled_tasks (
    id                    TEXT PRIMARY KEY NOT NULL,
    name                  TEXT NOT NULL,
    enabled               INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    schedule_json         TEXT NOT NULL,
    execution_location    TEXT NOT NULL CHECK (execution_location IN ('local', 'cloud')),
    project_id            TEXT NOT NULL REFERENCES projects(id),
    agent_ref             TEXT NOT NULL,
    model                 TEXT NULL,
    prompt                TEXT NOT NULL,
    revision              INTEGER NOT NULL CHECK (revision >= 1),
    approval_policy       TEXT NULL CHECK (
        approval_policy IS NULL OR approval_policy = 'full_auto_in_worktree'
    ),
    approval_consented_at INTEGER NULL,
    created_at            INTEGER NOT NULL,
    updated_at            INTEGER NOT NULL,
    is_deleted            INTEGER NOT NULL DEFAULT 0 CHECK (is_deleted IN (0, 1)),
    CHECK (
        (approval_policy IS NULL AND approval_consented_at IS NULL)
        OR (approval_policy IS NOT NULL AND approval_consented_at IS NOT NULL)
    )
);

CREATE INDEX idx_scheduled_tasks_visible_order
    ON scheduled_tasks(is_deleted, created_at, id);
"#];

const DOWN_STATEMENTS: &[&str] = &[r#"
DROP INDEX IF EXISTS idx_scheduled_tasks_visible_order;
DROP TABLE IF EXISTS scheduled_tasks;
"#];

/// Persists local Scheduled Task definitions while keeping execution ownership immutable in the
/// application update path and consent explicit in each stored row.
pub fn migration() -> Migration {
    Migration::new("0015", UP_STATEMENTS, DOWN_STATEMENTS)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use rusqlite::Connection;

    use super::{DOWN_STATEMENTS, UP_STATEMENTS};

    #[test]
    fn creates_constrained_scheduled_task_storage_and_rolls_back() {
        let connection = Connection::open_in_memory().expect("open database");
        connection
            .execute_batch("CREATE TABLE projects (id TEXT PRIMARY KEY NOT NULL);")
            .expect("create project fixture");
        connection
            .execute("INSERT INTO projects (id) VALUES ('project-1')", [])
            .expect("insert project fixture");
        for statement in UP_STATEMENTS {
            connection
                .execute_batch(statement)
                .expect("apply scheduled task migration");
        }

        connection
            .execute(
                "INSERT INTO scheduled_tasks (
                    id, name, enabled, schedule_json, execution_location, project_id,
                    agent_ref, model, prompt, revision, approval_policy,
                    approval_consented_at, created_at, updated_at, is_deleted
                 ) VALUES (
                    'task-1', 'Quality', 1, '{\"kind\":\"every_hours\",\"interval\":4}',
                    'local', 'project-1', 'ora/agent', NULL, 'Check quality', 1,
                    'full_auto_in_worktree', 10, 10, 10, 0
                 )",
                [],
            )
            .expect("insert scheduled task");
        let revision: i64 = connection
            .query_row(
                "SELECT revision FROM scheduled_tasks WHERE id = 'task-1'",
                [],
                |row| row.get(0),
            )
            .expect("read revision");
        assert_eq!(revision, 1);

        let invalid_consent = connection.execute(
            "INSERT INTO scheduled_tasks (
                id, name, enabled, schedule_json, execution_location, project_id,
                agent_ref, prompt, revision, approval_policy, created_at, updated_at
             ) VALUES (
                'task-2', 'Invalid', 0, '{}', 'local', 'project-1', 'ora/agent',
                'Prompt', 1, 'full_auto_in_worktree', 10, 10
             )",
            [],
        );
        assert!(invalid_consent.is_err());

        for statement in DOWN_STATEMENTS {
            connection
                .execute_batch(statement)
                .expect("roll back scheduled task migration");
        }
        let table_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'scheduled_tasks'",
                [],
                |row| row.get(0),
            )
            .expect("count tables");
        assert_eq!(table_count, 0);
    }
}
