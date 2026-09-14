use ora_application::{FileChange, RepositoryError};
use ora_domain::WorkflowNodeRunId;
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};

use super::engine_repository_error_from_database;
use crate::repository::RepositoryPool;

/// Serializes node file changes into the payload JSON array shared by complete and fail paths.
pub(super) fn file_changes_json(file_changes: &[FileChange]) -> serde_json::Value {
    serde_json::json!(
        file_changes
            .iter()
            .map(|change| {
                serde_json::json!({
                    "path": change.path,
                    "additions": change.additions,
                    "deletions": change.deletions,
                })
            })
            .collect::<Vec<_>>()
    )
}

/// Merges caller-supplied keys into an existing payload JSON object (empty map if NULL/invalid).
pub(super) fn merge_payload_keys(
    existing: Option<&str>,
    keys: impl IntoIterator<Item = (&'static str, serde_json::Value)>,
) -> Result<String, crate::DatabaseError> {
    let mut map = match existing.and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
    {
        Some(serde_json::Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    };
    for (key, value) in keys {
        map.insert(key.to_string(), value);
    }
    Ok(serde_json::Value::Object(map).to_string())
}

/// Builds the node-run `payload` blob: the ACP stop reason and incremental file changes, when any.
pub(super) fn complete_payload(
    stop_reason: Option<String>,
    file_changes: Vec<FileChange>,
) -> Option<String> {
    let mut payload = serde_json::Map::new();
    if let Some(reason) = stop_reason {
        payload.insert("stop_reason".to_string(), serde_json::json!(reason));
    }
    if !file_changes.is_empty() {
        payload.insert("file_changes".to_string(), file_changes_json(&file_changes));
    }
    if payload.is_empty() {
        return None;
    }
    Some(serde_json::Value::Object(payload).to_string())
}

/// Merges `payload.checkpoint` (and optional `checkpoint_error`) onto one node-run row.
///
/// A missing row is provenance-only and succeeds as a no-op so a late write cannot fail the node.
pub(super) fn record_node_checkpoint(
    pool: &RepositoryPool,
    node_run_id: &WorkflowNodeRunId,
    checkpoint: Option<&str>,
    checkpoint_error: Option<&str>,
    now: i64,
) -> Result<(), RepositoryError> {
    pool.with_connection_mut(|connection| {
        let transaction = Transaction::new(connection, TransactionBehavior::Immediate)?;
        let existing = transaction
            .query_row(
                "SELECT payload FROM workflow_node_runs WHERE id = ?1 AND is_deleted = 0",
                params![node_run_id.as_ref()],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?;
        let Some(existing) = existing else {
            return Ok(());
        };
        let mut keys = vec![("checkpoint", serde_json::json!(checkpoint))];
        if let Some(error) = checkpoint_error {
            keys.push(("checkpoint_error", serde_json::json!(error)));
        }
        let payload = merge_payload_keys(existing.as_deref(), keys)?;
        transaction.execute(
            "UPDATE workflow_node_runs SET payload = ?2, updated_at = ?3
             WHERE id = ?1 AND is_deleted = 0",
            params![node_run_id.as_ref(), payload, now],
        )?;
        transaction.commit()?;
        Ok(())
    })
    .map_err(engine_repository_error_from_database)
}
