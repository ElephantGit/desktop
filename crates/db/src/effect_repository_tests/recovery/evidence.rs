use super::*;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};

/// Audit evidence and journal closure must roll back together if durable audit cannot commit.
#[test]
fn recovery_rolls_back_when_detection_evidence_cannot_be_saved() {
    let (directory, pool, workspace) = fixture();
    let manifest = b"---\nname: review\ndescription: Reviews changes\n---\ncontent\n";
    let (repository, clock, target, _) =
        converged_target(directory.path(), &pool, &workspace, manifest);
    rewind_to_applied_leftover(&pool, &target);
    repository
        .quarantine_unfinished_operations(LocalTimestamp::from_millis(200))
        .unwrap();
    let before = capture(&pool, &target);
    let before_state = quarantine_state(&pool, &target);
    pool.with_connection(|connection| {
        connection.execute_batch(
            "CREATE TRIGGER refuse_recovery_audit BEFORE INSERT ON effect_audit_events
             WHEN NEW.event_kind = 'recovery_finalized'
             BEGIN SELECT RAISE(ABORT, 'simulated audit failure'); END;",
        )?;
        Ok(())
    })
    .unwrap();

    assert!(matches!(
        recover_target(&repository, &clock, &target),
        Err(RecoveryError::Repository(_))
    ));
    assert_eq!(capture(&pool, &target), before);
    assert_eq!(quarantine_state(&pool, &target), before_state);
    pool.with_connection(|connection| {
        connection.execute_batch("DROP TRIGGER refuse_recovery_audit")?;
        Ok(())
    })
    .unwrap();
    assert!(recover_target(&repository, &clock, &target).is_ok());
    assert_preserved(&pool, &target, "finalized", &before);
}

/// Captures exact quarantined timestamps and immutable intent before recovery changes phase.
pub(super) fn capture(pool: &crate::RepositoryPool, target: &EffectTargetId) -> Vec<Value> {
    pool.with_connection(|connection| {
        let mut statement = connection.prepare(
            "SELECT op.id, op.detected_at, op.expected_json, op.planned_json, op.payload_json,
                    op.prepared_at, op.applied_at
             FROM effect_operations op
             JOIN effect_reconcile_attempts attempt ON attempt.id = op.attempt_id
             WHERE attempt.target_id = ?1 AND op.phase = 'recovery_required' ORDER BY op.id",
        )?;
        let rows = statement.query_map(params![target.as_str()], |row| {
            Ok(json!({
                "operation_id": row.get::<_, String>(0)?,
                "detected_at": row.get::<_, i64>(1)?,
                "expected": row.get::<_, String>(2)?,
                "planned": row.get::<_, String>(3)?,
                "payload": row.get::<_, String>(4)?,
                "prepared_at": row.get::<_, i64>(5)?,
                "applied_at": row.get::<_, Option<i64>>(6)?,
            }))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    })
    .unwrap()
}

/// Closing either outcome clears active phase time but retains exact detection in durable audit.
pub(super) fn assert_preserved(
    pool: &crate::RepositoryPool,
    target: &EffectTargetId,
    outcome: &str,
    before: &[Value],
) {
    assert!(!before.is_empty());
    pool.with_connection(|connection| {
        let audit: String = connection.query_row(
            "SELECT payload_json FROM effect_audit_events
             WHERE subject_id = ?1 AND event_kind = 'recovery_finalized'",
            params![target.as_str()], |row| row.get(0),
        )?;
        let audit: Value = serde_json::from_str(&audit).unwrap();
        let mut actual = audit["operations"].as_array().expect("per-operation detection evidence").clone();
        actual.sort_by_key(|value| value["operation_id"].as_str().unwrap().to_owned());
        let expected: Vec<_> = before.iter().map(|row| json!({
            "operation_id": row["operation_id"],
            "detected_at": row["detected_at"],
            "outcome": outcome,
        })).collect();
        assert_eq!(actual, expected);
        for snapshot in before {
            let row = connection.query_row(
                "SELECT phase, detected_at, expected_json, planned_json, payload_json, prepared_at, applied_at
                 FROM effect_operations WHERE id = ?1",
                params![snapshot["operation_id"].as_str().unwrap()],
                |row| Ok(json!({
                    "phase": row.get::<_, String>(0)?,
                    "detected_at": row.get::<_, Option<i64>>(1)?,
                    "expected": row.get::<_, String>(2)?,
                    "planned": row.get::<_, String>(3)?,
                    "payload": row.get::<_, String>(4)?,
                    "prepared_at": row.get::<_, i64>(5)?,
                    "applied_at": row.get::<_, Option<i64>>(6)?,
                })),
            )?;
            assert_eq!(row, json!({
                "phase": outcome,
                "detected_at": null,
                "expected": snapshot["expected"],
                "planned": snapshot["planned"],
                "payload": snapshot["payload"],
                "prepared_at": snapshot["prepared_at"],
                "applied_at": snapshot["applied_at"],
            }));
        }
        Ok(())
    }).unwrap();
}
