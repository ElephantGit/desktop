use ora_domain::{FULL_AUTO_IN_WORKTREE_POLICY, ScheduledTaskApproval};

use crate::DatabaseError;

/// Splits one persisted consent fact into its SQLite columns.
pub(crate) fn approval_columns(
    approval: Option<&ScheduledTaskApproval>,
) -> (Option<String>, Option<i64>) {
    approval
        .map(|approval| (Some(approval.policy.clone()), Some(approval.consented_at)))
        .unwrap_or((None, None))
}

/// Rebuilds one persisted consent fact from its SQLite columns.
pub(crate) fn map_approval_columns(
    policy: Option<String>,
    consented_at: Option<i64>,
) -> Result<Option<ScheduledTaskApproval>, DatabaseError> {
    match (policy, consented_at) {
        (None, None) => Ok(None),
        (Some(policy), Some(consented_at)) if policy == FULL_AUTO_IN_WORKTREE_POLICY => Ok(Some(
            ScheduledTaskApproval::full_auto_in_worktree(consented_at),
        )),
        (policy, consented_at) => Err(DatabaseError::CorruptScheduledTaskApproval(format!(
            "policy={policy:?}, consented_at={consented_at:?}"
        ))),
    }
}
