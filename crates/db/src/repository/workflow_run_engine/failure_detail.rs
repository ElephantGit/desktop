use ora_application::{NodeFailure, NodeFailureDetail, NodeFailureKind};
use ora_domain::{SessionStatus, WorkflowNodeStatus, WorkflowRunId, WorkflowRunStatus};
use rusqlite::{Transaction, params};

/// Error written to node runs and runs interrupted by a backend restart.
const INTERRUPTED_BY_RESTART: &str = r#"{"reason":"interrupted_by_restart"}"#;

/// Counts prior soft-deleted attempts of this node in the same run.
fn deleted_attempt_count(
    transaction: &Transaction<'_>,
    run_id: &str,
    node_id: &str,
) -> Result<u32, crate::DatabaseError> {
    let count: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM workflow_node_runs WHERE run_id = ?1 AND node_id = ?2 AND is_deleted = 1",
        params![run_id, node_id],
        |row| row.get(0),
    )?;
    Ok(u32::try_from(count).unwrap_or(u32::MAX))
}

/// Merges `error_detail` into an existing payload JSON object (empty map if NULL/invalid).
fn merge_error_detail(
    payload: Option<&str>,
    detail: &NodeFailureDetail,
) -> Result<String, crate::DatabaseError> {
    let mut map = match payload.and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
    {
        Some(serde_json::Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    };
    map.insert("error_detail".to_string(), serde_json::to_value(detail)?);
    Ok(serde_json::Value::Object(map).to_string())
}

/// Marks one node-run `Failed` and writes `payload.error_detail` in the same UPDATE.
pub(super) fn persist_failed_node_run(
    transaction: &Transaction<'_>,
    node_run_id: &str,
    run_id: &str,
    node_id: &str,
    failure: &NodeFailure,
    current_payload: Option<&str>,
    now: i64,
) -> Result<(), crate::DatabaseError> {
    let attempt = deleted_attempt_count(transaction, run_id, node_id)?.saturating_add(1);
    let detail = NodeFailureDetail {
        kind: failure.kind,
        message: failure.message.clone(),
        source_chain: failure.source_chain.clone(),
        attempt,
        resumable: failure.kind.resumable(),
        recorded_at: now,
    };
    let payload = merge_error_detail(current_payload, &detail)?;
    transaction.execute(
        "UPDATE workflow_node_runs SET status = ?2, error = ?3, output = ?4, payload = ?5, finished_at = ?6, updated_at = ?6
         WHERE id = ?1 AND is_deleted = 0",
        params![
            node_run_id,
            WorkflowNodeStatus::Failed.database_value(),
            &failure.message,
            &failure.output,
            payload,
            now,
        ],
    )?;
    Ok(())
}

/// Fails non-terminal node runs of one run that still had a generating node at restart.
pub(super) fn fail_orphaned_run(
    transaction: &Transaction<'_>,
    run_id: &WorkflowRunId,
    now: i64,
) -> Result<(), crate::DatabaseError> {
    // An awaiting (`Pending`) node is parked on human input, not computing: a restart must not
    // destroy it. Only a run that has a `Running` (actively generating) node fails, and it takes
    // every non-terminal node with it.
    let has_generating: bool = transaction.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM workflow_node_runs
            WHERE run_id = ?1 AND status = ?2 AND is_deleted = 0
         )",
        params![
            run_id.as_ref(),
            WorkflowNodeStatus::Running.database_value()
        ],
        |row| row.get(0),
    )?;
    if !has_generating {
        return Ok(());
    }
    let nodes = {
        let mut statement = transaction.prepare(
            "SELECT id, node_id, payload FROM workflow_node_runs
             WHERE run_id = ?1 AND status IN (?2, ?3) AND is_deleted = 0",
        )?;
        let rows = statement.query_map(
            params![
                run_id.as_ref(),
                WorkflowNodeStatus::Pending.database_value(),
                WorkflowNodeStatus::Running.database_value(),
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    let failure = NodeFailure::new(
        NodeFailureKind::InterruptedByRestart,
        INTERRUPTED_BY_RESTART,
    );
    for (id, node_id, payload) in nodes {
        persist_failed_node_run(
            transaction,
            &id,
            run_id.as_ref(),
            &node_id,
            &failure,
            payload.as_deref(),
            now,
        )?;
    }
    transaction.execute(
        "UPDATE workflow_runs SET run_status = ?2, error = ?3, finished_at = ?4, updated_at = ?4
         WHERE id = ?1 AND run_status = ?5 AND is_deleted = 0",
        params![
            run_id.as_ref(),
            WorkflowRunStatus::Failed.database_value(),
            INTERRUPTED_BY_RESTART,
            now,
            WorkflowRunStatus::Running.database_value(),
        ],
    )?;
    transaction.execute(
        "UPDATE sessions SET status = ?2, updated_at = ?3
         WHERE workspace_id = (SELECT workspace_id FROM workflow_runs WHERE id = ?1 AND is_deleted = 0)
           AND status = ?4 AND is_deleted = 0",
        params![
            run_id.as_ref(),
            SessionStatus::Stopped.database_value(),
            now,
            SessionStatus::Running.database_value(),
        ],
    )?;
    Ok(())
}
