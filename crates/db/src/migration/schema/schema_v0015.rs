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
    worktree_path         TEXT NULL,
    failure_reason        TEXT NULL CHECK (
        (status = 'failed' AND failure_reason IN (
                'target_unavailable', 'model_unavailable', 'worktree_failed',
                'approval_policy_invalid', 'timeout', 'execution_error'
            ))
        OR (status <> 'failed' AND failure_reason IS NULL)
    ),
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

CREATE TABLE scheduled_occurrences (
    task_id       TEXT NOT NULL REFERENCES scheduled_tasks(id),
    scheduled_at  INTEGER NOT NULL,
    outcome       TEXT NOT NULL CHECK (outcome IN ('accepted', 'missed')),
    missed_reason TEXT NULL CHECK (
        missed_reason IS NULL OR missed_reason IN ('app_not_running', 'late_trigger')
    ),
    observed_at   INTEGER NOT NULL,
    PRIMARY KEY (task_id, scheduled_at),
    CHECK (
        (outcome = 'accepted' AND missed_reason IS NULL)
        OR (outcome = 'missed' AND missed_reason IS NOT NULL)
    )
);
"#];

const DOWN_STATEMENTS: &[&str] = &[r#"
DROP TABLE IF EXISTS scheduled_occurrences;
DROP INDEX IF EXISTS idx_scheduled_runs_task_history;
DROP TABLE IF EXISTS scheduled_runs;
DROP INDEX IF EXISTS idx_scheduled_tasks_visible_order;
DROP TABLE IF EXISTS scheduled_tasks;
"#];

/// Persists the local Scheduled Task definition, occurrence identity, and immutable run snapshot.
pub fn migration() -> Migration {
    Migration::new("0015", UP_STATEMENTS, DOWN_STATEMENTS)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use rusqlite::Connection;

    use super::{DOWN_STATEMENTS, UP_STATEMENTS};

    #[test]
    fn creates_final_scheduled_task_schema_and_rolls_back() {
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

        connection
            .execute(
                "INSERT INTO scheduled_occurrences
                 (task_id, scheduled_at, outcome, missed_reason, observed_at)
                 VALUES ('task-1', 100, 'accepted', NULL, 10)",
                [],
            )
            .expect("insert accepted occurrence");
        connection
            .execute(
                "INSERT INTO scheduled_runs (
                    id, task_id, scheduled_at, task_revision, project_id, agent_ref,
                    prompt, approval_policy, approval_consented_at, status, worktree_path,
                    failure_reason, created_at, updated_at
                 ) VALUES (
                    'run-1', 'task-1', 100, 1, 'project-1', 'ora/agent', 'Check quality',
                    'full_auto_in_worktree', 10, 'failed', 'worktrees/run-1', 'timeout', 10, 10
                 )",
                [],
            )
            .expect("insert scheduled run with lifecycle details");
        assert!(
            connection
                .execute(
                    "UPDATE scheduled_runs SET failure_reason = 'unknown' WHERE id = 'run-1'",
                    [],
                )
                .is_err()
        );
        assert!(
            connection
                .execute(
                    "INSERT INTO scheduled_occurrences
                 (task_id, scheduled_at, outcome, missed_reason, observed_at)
                 VALUES ('task-1', 100, 'missed', 'late_trigger', 11)",
                    [],
                )
                .is_err()
        );

        for statement in DOWN_STATEMENTS {
            connection
                .execute_batch(statement)
                .expect("roll back scheduled task migration");
        }
        let table_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'table'
                   AND name IN ('scheduled_tasks', 'scheduled_runs', 'scheduled_occurrences')",
                [],
                |row| row.get(0),
            )
            .expect("count tables");
        assert_eq!(table_count, 0);
    }
}
