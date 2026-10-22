use super::Migration;

const UP_STATEMENTS: &[&str] =
    &["ALTER TABLE scheduled_runs ADD COLUMN worktree_base_branch TEXT NULL;"];

const DOWN_STATEMENTS: &[&str] = &["ALTER TABLE scheduled_runs DROP COLUMN worktree_base_branch;"];

/// Persists the branch fact selected at occurrence admission for later safe execution.
pub fn migration() -> Migration {
    Migration::new("0017", UP_STATEMENTS, DOWN_STATEMENTS)
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::UP_STATEMENTS;

    #[test]
    fn adds_a_nullable_snapshot_branch_without_rewriting_history() {
        let connection = Connection::open_in_memory().expect("open database");
        connection
            .execute_batch(
                "CREATE TABLE scheduled_runs (id TEXT PRIMARY KEY NOT NULL);\
                 INSERT INTO scheduled_runs (id) VALUES ('legacy-run');",
            )
            .expect("seed v0016-shaped table");
        for statement in UP_STATEMENTS {
            connection
                .execute_batch(statement)
                .expect("apply migration");
        }

        let branch: Option<String> = connection
            .query_row(
                "SELECT worktree_base_branch FROM scheduled_runs WHERE id = 'legacy-run'",
                [],
                |row| row.get(0),
            )
            .expect("read migrated history");
        assert_eq!(branch, None);
    }
}
