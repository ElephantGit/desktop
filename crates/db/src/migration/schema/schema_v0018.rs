use super::Migration;

const UP_STATEMENTS: &[&str] =
    &["ALTER TABLE scheduled_runs ADD COLUMN worktree_task_id TEXT NULL;"];

const DOWN_STATEMENTS: &[&str] = &["ALTER TABLE scheduled_runs DROP COLUMN worktree_task_id;"];

/// Persists the task-owned worktree a run executed in so history can open or discard
/// the retained worktree and retention can bound it after a restart.
pub fn migration() -> Migration {
    Migration::new("0018", UP_STATEMENTS, DOWN_STATEMENTS)
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::UP_STATEMENTS;

    #[test]
    fn adds_a_nullable_worktree_task_link_without_rewriting_history() {
        let connection = Connection::open_in_memory().expect("open database");
        connection
            .execute_batch(
                "CREATE TABLE scheduled_runs (id TEXT PRIMARY KEY NOT NULL);\
                 INSERT INTO scheduled_runs (id) VALUES ('legacy-run');",
            )
            .expect("seed v0017-shaped table");
        for statement in UP_STATEMENTS {
            connection
                .execute_batch(statement)
                .expect("apply migration");
        }

        let task_id: Option<String> = connection
            .query_row(
                "SELECT worktree_task_id FROM scheduled_runs WHERE id = 'legacy-run'",
                [],
                |row| row.get(0),
            )
            .expect("read migrated history");
        assert_eq!(task_id, None);
    }
}
