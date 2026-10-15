use super::Migration;

const UP_STATEMENTS: &[&str] = &[r#"
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

INSERT INTO scheduled_occurrences (task_id, scheduled_at, outcome, missed_reason, observed_at)
SELECT task_id, scheduled_at, 'accepted', NULL, created_at
FROM scheduled_runs;
"#];

const DOWN_STATEMENTS: &[&str] = &[r#"
DROP TABLE IF EXISTS scheduled_occurrences;
"#];

/// Unifies accepted and missed outcomes behind one persistent occurrence identity.
pub fn migration() -> Migration {
    Migration::new("0017", UP_STATEMENTS, DOWN_STATEMENTS)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use rusqlite::Connection;

    use super::{DOWN_STATEMENTS, UP_STATEMENTS};

    #[test]
    fn backfills_runs_and_prevents_a_second_outcome_for_the_same_occurrence() {
        let connection = Connection::open_in_memory().expect("open database");
        connection
            .execute_batch(
                "CREATE TABLE scheduled_tasks (id TEXT PRIMARY KEY NOT NULL);\n\
                 CREATE TABLE scheduled_runs (\n\
                    id TEXT PRIMARY KEY NOT NULL, task_id TEXT NOT NULL,\n\
                    scheduled_at INTEGER NOT NULL, created_at INTEGER NOT NULL\n\
                 );\n\
                 INSERT INTO scheduled_tasks (id) VALUES ('task-1');\n\
                 INSERT INTO scheduled_runs (id, task_id, scheduled_at, created_at)\n\
                 VALUES ('run-1', 'task-1', 100, 101);",
            )
            .expect("create prior schema fixture");
        for statement in UP_STATEMENTS {
            connection
                .execute_batch(statement)
                .expect("apply occurrence migration");
        }

        let outcome: String = connection
            .query_row(
                "SELECT outcome FROM scheduled_occurrences
                 WHERE task_id = 'task-1' AND scheduled_at = 100",
                [],
                |row| row.get(0),
            )
            .expect("read backfilled occurrence");
        assert_eq!(outcome, "accepted");
        assert!(
            connection
                .execute(
                    "INSERT INTO scheduled_occurrences
                     (task_id, scheduled_at, outcome, missed_reason, observed_at)
                     VALUES ('task-1', 100, 'missed', 'late_trigger', 102)",
                    [],
                )
                .is_err()
        );

        for statement in DOWN_STATEMENTS {
            connection
                .execute_batch(statement)
                .expect("roll back occurrence migration");
        }
    }
}
