use super::Migration;

const UP_STATEMENTS: &[&str] = &[r#"
ALTER TABLE sessions ADD COLUMN mcp_selection TEXT NOT NULL
    DEFAULT '{"mode":"automatic"}' CHECK (json_valid(mcp_selection));

-- Workflow sessions own a frozen explicit permission set. Invalid legacy IDs are deliberately
-- omitted so migration can never turn malformed authoring data into runtime authority.
UPDATE sessions AS session
SET mcp_selection = COALESCE((
    SELECT json_object(
        'mode', 'explicit',
        'pluginIds', json(COALESCE((
            SELECT json_group_array(binding.mcp_id)
            FROM (
                SELECT json_extract(mcp.value, '$.mcpId') AS mcp_id
                FROM json_each(COALESCE(json_extract(node.value, '$.data.agentConfig.mcps'), '[]')) AS mcp
                WHERE json_extract(mcp.value, '$.enabled') = 1
                  AND json_type(mcp.value, '$.mcpId') = 'text'
                  AND instr(json_extract(mcp.value, '$.mcpId'), '/') > 1
                  AND instr(
                      substr(
                          json_extract(mcp.value, '$.mcpId'),
                          instr(json_extract(mcp.value, '$.mcpId'), '/') + 1
                      ),
                      '/'
                  ) = 0
                  AND length(CAST(substr(
                      json_extract(mcp.value, '$.mcpId'),
                      1,
                      instr(json_extract(mcp.value, '$.mcpId'), '/') - 1
                  ) AS BLOB)) <= 33
                  AND substr(
                      json_extract(mcp.value, '$.mcpId'),
                      1,
                      instr(json_extract(mcp.value, '$.mcpId'), '/') - 1
                  ) NOT IN ('.', '..')
                  AND substr(
                      json_extract(mcp.value, '$.mcpId'),
                      instr(json_extract(mcp.value, '$.mcpId'), '/') + 1
                  ) NOT IN ('', '.', '..')
                  AND substr(
                      json_extract(mcp.value, '$.mcpId'),
                      1,
                      instr(json_extract(mcp.value, '$.mcpId'), '/') - 1
                  ) NOT GLOB '*[^a-z0-9.-]*'
                  AND substr(
                      json_extract(mcp.value, '$.mcpId'),
                      instr(json_extract(mcp.value, '$.mcpId'), '/') + 1
                  ) NOT GLOB '*[^a-z0-9.-]*'
                ORDER BY mcp_id
            ) AS binding
        ), '[]'))
    )
    FROM workflow_node_runs AS node_run
    JOIN workflow_runs AS workflow_run ON workflow_run.id = node_run.run_id
    JOIN workflow_snapshots AS snapshot ON snapshot.id = workflow_run.snapshot_id
    JOIN json_each(
        CASE WHEN json_valid(snapshot.graph) THEN snapshot.graph ELSE '{"nodes":[]}' END,
        '$.nodes'
    ) AS node
      ON json_extract(node.value, '$.id') = node_run.node_id
    WHERE node_run.session_id = session.id
    ORDER BY node_run.created_at DESC, node_run.id DESC
    LIMIT 1
), '{"mode":"explicit","pluginIds":[]}')
WHERE EXISTS (
    SELECT 1
    FROM workflow_node_runs AS node_run
    WHERE node_run.session_id = session.id
);
"#];

const DOWN_STATEMENTS: &[&str] = &[r#"
ALTER TABLE sessions DROP COLUMN mcp_selection;
"#];

/// Persists each session's MCP authority and freezes historical workflow selections in place.
pub fn migration() -> Migration {
    Migration::new("0011", UP_STATEMENTS, DOWN_STATEMENTS)
}

#[cfg(test)]
mod tests {
    use super::{DOWN_STATEMENTS, UP_STATEMENTS};
    use pretty_assertions::assert_eq;
    use rusqlite::Connection;

    /// Upgrade preserves ordinary discovery and freezes valid workflow permissions fail closed.
    #[test]
    fn backfills_session_owned_mcp_selection_and_rolls_back() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                r#"
                CREATE TABLE sessions (id TEXT PRIMARY KEY);
                CREATE TABLE workflow_snapshots (id TEXT PRIMARY KEY, graph TEXT NOT NULL);
                CREATE TABLE workflow_runs (
                    id TEXT PRIMARY KEY,
                    snapshot_id TEXT NOT NULL
                );
                CREATE TABLE workflow_node_runs (
                    id TEXT PRIMARY KEY,
                    run_id TEXT NOT NULL,
                    node_id TEXT NOT NULL,
                    session_id TEXT,
                    created_at INTEGER NOT NULL
                );
                INSERT INTO sessions (id) VALUES
                    ('ordinary'), ('workflow'), ('orphan'), ('malformed');
                INSERT INTO workflow_snapshots (id, graph) VALUES (
                    'snapshot',
                    '{"nodes":[{"id":"agent","data":{"kind":"agent","agentConfig":{"mcps":[{"mcpId":"official/github","enabled":true},{"mcpId":"github","enabled":true},{"mcpId":"local/disabled","enabled":false}]}}}],"edges":[]}'
                );
                INSERT INTO workflow_runs (id, snapshot_id) VALUES ('run', 'snapshot');
                INSERT INTO workflow_snapshots (id, graph) VALUES ('bad-snapshot', '{');
                INSERT INTO workflow_runs (id, snapshot_id) VALUES ('bad-run', 'bad-snapshot');
                INSERT INTO workflow_node_runs (id, run_id, node_id, session_id, created_at)
                    VALUES ('node-run', 'run', 'agent', 'workflow', 1);
                INSERT INTO workflow_node_runs (id, run_id, node_id, session_id, created_at)
                    VALUES ('orphan-run', 'missing', 'agent', 'orphan', 1);
                INSERT INTO workflow_node_runs (id, run_id, node_id, session_id, created_at)
                    VALUES ('bad-node-run', 'bad-run', 'agent', 'malformed', 1);
                "#,
            )
            .unwrap();

        connection.execute_batch(UP_STATEMENTS[0]).unwrap();

        let rows = connection
            .prepare("SELECT id, mcp_selection FROM sessions ORDER BY id")
            .unwrap()
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            rows,
            vec![
                (
                    "malformed".to_string(),
                    r#"{"mode":"explicit","pluginIds":[]}"#.to_string()
                ),
                (
                    "ordinary".to_string(),
                    r#"{"mode":"automatic"}"#.to_string()
                ),
                (
                    "orphan".to_string(),
                    r#"{"mode":"explicit","pluginIds":[]}"#.to_string()
                ),
                (
                    "workflow".to_string(),
                    r#"{"mode":"explicit","pluginIds":["official/github"]}"#.to_string()
                ),
            ]
        );

        connection.execute_batch(DOWN_STATEMENTS[0]).unwrap();
        let columns = connection
            .prepare("PRAGMA table_info(sessions)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(columns, vec!["id"]);
    }
}
