//! Transactional scope identity owned by the workflow execution repository.

use ora_domain::WorkflowRunId;
use rusqlite::{Transaction, params};

/// A restart gets a fresh root identity while historical node bindings remain immutable.
pub(super) fn restart_root_scope(
    transaction: &Transaction<'_>,
    run_id: &WorkflowRunId,
    now: i64,
) -> Result<(), crate::DatabaseError> {
    transaction.execute(
        "UPDATE workflow_execution_scopes SET status = 4, updated_at = ?2
         WHERE run_id = ?1 AND parent_loop_node_run_id IS NOT NULL AND status IN (0, 1)",
        params![run_id.as_ref(), now],
    )?;
    let scope_id: String = transaction.query_row(
        "INSERT INTO workflow_execution_scopes (id, run_id, created_at, updated_at)
         VALUES ('root:' || lower(hex(randomblob(16))), ?1, ?2, ?2) RETURNING id",
        params![run_id.as_ref(), now],
        |row| row.get(0),
    )?;
    transaction.execute(
        "UPDATE workflow_run_root_scopes SET scope_id = ?2 WHERE run_id = ?1",
        params![run_id.as_ref(), scope_id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests;
