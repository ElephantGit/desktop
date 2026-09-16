use crate::{
    DatabaseBootstrapper, DatabaseLocation, SqliteWorkflowRunEngineRepository,
    default_migration_catalog, test_clock::TestClock,
};
use ora_application::{
    NodeRunToStart, RestartWorkflowRunResult, StartWorkflowRunResult, WorkflowRunEngineRepository,
};
use ora_domain::{WorkflowNodeRunId, WorkflowRunId};
use ora_logging::with_trace_logging;
use pretty_assertions::assert_eq;

/// The production restart transaction rotates roots without rewriting old node scope identities.
#[test]
fn restart_rotates_root_and_preserves_node_history() {
    with_trace_logging(|| {
        let pool = DatabaseBootstrapper::new(TestClock::new(1))
            .bootstrap_repository_pool(
                &DatabaseLocation::in_memory(),
                &default_migration_catalog().unwrap(),
            )
            .unwrap();
        pool.with_connection(|connection| {
            connection.execute_batch(r#"
                INSERT INTO projects (id, name, created_at, updated_at) VALUES ('project', 'Project', 1, 1);
                INSERT INTO workspace_locations (id, location_kind, locator_json, created_at, updated_at)
                VALUES ('location', 'local_filesystem', '{}', 1, 1);
                INSERT INTO workspaces (id, project_id, workspace_kind, location_id, created_at, updated_at)
                VALUES ('workspace', 'project', 'main', 'location', 1, 1);
                INSERT INTO workflows (id, name, created_at, updated_at) VALUES ('workflow', 'Workflow', 1, 1);
                INSERT INTO workflow_snapshots (id, workflow_id, version, graph, created_at)
                VALUES ('snapshot', 'workflow', '1', '{}', 1);
                INSERT INTO workflow_runs (id, workspace_id, workflow_id, snapshot_id, name, run_status, created_at, updated_at)
                VALUES ('run', 'workspace', 'workflow', 'snapshot', 'Run', 2, 1, 2);
                INSERT INTO workflow_node_runs (id, run_id, node_id, node_type, status, created_at, updated_at)
                VALUES ('old-start', 'run', 'start', 'start', 2, 1, 2);
            "#)?;
            Ok(())
        }).unwrap();
        let repository = SqliteWorkflowRunEngineRepository::new(pool.clone());
        let run_id = WorkflowRunId::new("run");
        assert_eq!(
            repository.restart_run(&run_id, /*now*/ 3).unwrap(),
            RestartWorkflowRunResult::Restarted
        );
        let root = pool
            .with_connection(|connection| {
                Ok(connection.query_row(
                    "SELECT scope_id FROM workflow_run_root_scopes WHERE run_id = 'run'",
                    [],
                    |row| row.get::<_, String>(0),
                )?)
            })
            .unwrap();
        assert_ne!(root, "root:run");
        let start = NodeRunToStart {
            id: WorkflowNodeRunId::new("new-start"),
            scope_id: ora_domain::WorkflowScopeId::new(root.clone()),
            node_id: "start".into(),
            node_type: "start".into(),
            input: None,
        };
        let stale = NodeRunToStart {
            id: WorkflowNodeRunId::new("stale-start"),
            scope_id: ora_domain::WorkflowScopeId::new("root:run"),
            ..start.clone()
        };
        assert!(repository.start_run(&run_id, &stale, /*now*/ 4).is_err());
        assert_eq!(repository.find_node_run_by_id(&stale.id).unwrap(), None);
        assert_eq!(
            repository.start_run(&run_id, &start, /*now*/ 4).unwrap(),
            StartWorkflowRunResult::Started
        );
        assert_eq!(
            repository
                .find_node_run_by_id(&start.id)
                .unwrap()
                .unwrap()
                .scope_id,
            start.scope_id
        );
        assert_eq!(
            repository
                .list_node_runs(&run_id)
                .unwrap()
                .iter()
                .map(|node| (&node.id, &node.scope_id))
                .collect::<Vec<_>>(),
            vec![(&start.id, &start.scope_id)]
        );
        let nodes = pool
            .with_connection(|connection| {
                Ok(connection
                    .prepare("SELECT id, scope_id, is_deleted FROM workflow_node_runs ORDER BY id")?
                    .query_map([], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, i64>(2)?,
                        ))
                    })?
                    .collect::<Result<Vec<_>, _>>()?)
            })
            .unwrap();
        assert_eq!(
            nodes,
            vec![
                ("new-start".into(), root.clone(), 0),
                ("old-start".into(), "root:run".into(), 1)
            ]
        );
        assert_eq!(
            repository.restart_run(&run_id, /*now*/ 5).unwrap(),
            RestartWorkflowRunResult::NotRestartable
        );
        let unchanged = pool
            .with_connection(|connection| {
                Ok(connection.query_row(
                    "SELECT scope_id FROM workflow_run_root_scopes WHERE run_id = 'run'",
                    [],
                    |row| row.get::<_, String>(0),
                )?)
            })
            .unwrap();
        assert_eq!(unchanged, root);
    });
}
