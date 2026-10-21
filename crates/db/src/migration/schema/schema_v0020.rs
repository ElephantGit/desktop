use super::Migration;

const UP_STATEMENTS: &[&str] = &[
    "ALTER TABLE scheduled_runs ADD COLUMN verdict TEXT NULL;",
    "ALTER TABLE scheduled_runs ADD COLUMN verdict_detail TEXT NULL;",
];

/// Adds the nullable business verdict and its free-text detail to run rows, so a run
/// can carry a machine-readable conclusion independently of its execution status.
/// Existing rows keep `NULL` and are read as "not reported".
pub fn migration() -> Migration {
    Migration::new("0020", UP_STATEMENTS, &[])
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::UP_STATEMENTS;

    #[test]
    fn adds_nullable_verdict_columns_without_rewriting_history() {
        let connection = Connection::open_in_memory().expect("open database");
        connection
            .execute_batch(
                "CREATE TABLE scheduled_runs (id TEXT PRIMARY KEY NOT NULL);\
                 INSERT INTO scheduled_runs (id) VALUES ('legacy-run');",
            )
            .expect("seed v0019-shaped table");
        for statement in UP_STATEMENTS {
            connection
                .execute_batch(statement)
                .expect("apply migration");
        }

        let (verdict, detail): (Option<String>, Option<String>) = connection
            .query_row(
                "SELECT verdict, verdict_detail FROM scheduled_runs WHERE id = 'legacy-run'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("read migrated history");
        assert_eq!(verdict, None);
        assert_eq!(detail, None);
    }
}
