use super::Migration;

const UP_STATEMENTS: &[&str] = &[
    "ALTER TABLE scheduled_tasks ADD COLUMN schedule_anchor_at INTEGER NULL;",
    "ALTER TABLE scheduled_tasks ADD COLUMN worktree_base_branch TEXT NULL;",
    r#"
UPDATE scheduled_tasks
SET schedule_anchor_at = created_at
WHERE enabled = 1
  AND json_extract(schedule_json, '$.kind') = 'every_hours';
"#,
    r#"
UPDATE scheduled_tasks
SET worktree_base_branch = (
    SELECT default_branch
    FROM projects
    WHERE projects.id = scheduled_tasks.project_id
)
WHERE worktree_base_branch IS NULL
  AND EXISTS (
      SELECT 1
      FROM projects
      WHERE projects.id = scheduled_tasks.project_id
        AND default_branch IS NOT NULL
        AND trim(default_branch) <> ''
  );
"#,
];

/// Adds scheduler-only facts without guessing a branch when older project metadata lacks one.
pub fn migration() -> Migration {
    Migration::new("0016", UP_STATEMENTS, &[])
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use rusqlite::Connection;

    use super::UP_STATEMENTS;

    #[test]
    fn backfills_only_deterministic_scheduler_facts() {
        let connection = Connection::open_in_memory().expect("open database");
        connection
            .execute_batch(
                "CREATE TABLE projects (id TEXT PRIMARY KEY NOT NULL, default_branch TEXT NULL);\
                 CREATE TABLE scheduled_tasks (\
                    id TEXT PRIMARY KEY NOT NULL, project_id TEXT NOT NULL, enabled INTEGER NOT NULL,\
                    schedule_json TEXT NOT NULL, created_at INTEGER NOT NULL\
                 );\
                 INSERT INTO projects (id, default_branch) VALUES ('with-branch', 'main');\
                 INSERT INTO projects (id, default_branch) VALUES ('without-branch', NULL);\
                 INSERT INTO scheduled_tasks (id, project_id, enabled, schedule_json, created_at)\
                 VALUES ('hourly', 'with-branch', 1, '{\"kind\":\"every_hours\",\"interval\":4}', 101);\
                 INSERT INTO scheduled_tasks (id, project_id, enabled, schedule_json, created_at)\
                 VALUES ('daily', 'without-branch', 1, '{\"kind\":\"daily\",\"hour\":9,\"minute\":0}', 102);",
            )
            .expect("seed v0015-shaped tables");
        for statement in UP_STATEMENTS {
            connection
                .execute_batch(statement)
                .expect("apply migration");
        }

        let hourly: (Option<i64>, Option<String>) = connection
            .query_row(
                "SELECT schedule_anchor_at, worktree_base_branch FROM scheduled_tasks WHERE id = 'hourly'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("read migrated hourly task");
        assert_eq!(hourly, (Some(101), Some("main".to_string())));
        let daily: (Option<i64>, Option<String>) = connection
            .query_row(
                "SELECT schedule_anchor_at, worktree_base_branch FROM scheduled_tasks WHERE id = 'daily'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("read migrated daily task");
        assert_eq!(daily, (None, None));
    }
}
