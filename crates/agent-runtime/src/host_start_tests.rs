//! Covers what a host that starts sessions on its own relies on: a session identity it chose can
//! never join an existing conversation, and waiting for an agent ends with an answer.

use crate::ErrorClassification;
use crate::MemorySessionStore;
use crate::host::SessionStore;
use crate::test_host::{TestRuntime, test_runtime};
use ora_contracts::{EmptyErrorParams, PublicError, StartSessionRequest};
use ora_domain::{
    AgentRef, AuditFields, Session, SessionId, SessionMcpSelection, SessionStatus, WorkspaceId,
};
use ora_scheduler::Scheduler;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

/// Starting under an identity the store already holds is refused before any agent is reached,
/// and the stored conversation is left exactly as it was.
#[tokio::test]
async fn an_identity_already_in_use_is_refused() {
    let temporary = TempDir::new().expect("create test directory");
    let scheduler = Scheduler::new(chrono_tz::UTC);
    let store = MemorySessionStore::default();
    let agent_ref = AgentRef::parse("official/ora-space.echo").expect("agent identity");
    let existing = store
        .create_session(Session::new(
            SessionId::new("execution-1"),
            WorkspaceId::new("workspace-1"),
            agent_ref.clone(),
            "provider-1",
            SessionStatus::Stopped,
            SessionMcpSelection::Automatic,
            AuditFields::new(1, 1, false),
        ))
        .expect("seed session");
    let TestRuntime { manager, .. } = test_runtime(
        temporary.path(),
        Vec::new(),
        store.clone(),
        scheduler.clone(),
    );

    let error = manager
        .start_session_with_id(
            SessionId::new("execution-1"),
            StartSessionRequest {
                workspace_id: "workspace-1".to_string(),
                agent_ref: agent_ref.to_string(),
                model: None,
            },
        )
        .await
        .expect_err("an identity in use is refused");

    assert_eq!(
        (
            error.classification(),
            store.list_sessions().expect("list sessions"),
        ),
        (ErrorClassification::Conflict, vec![existing]),
    );
    scheduler.shutdown().await;
}

/// Waiting for an agent nothing supplies answers at once instead of waiting forever, because no
/// supervisor exists to ever report it ready.
#[tokio::test]
async fn waiting_for_an_agent_that_is_not_installed_fails_at_once() {
    let temporary = TempDir::new().expect("create test directory");
    let scheduler = Scheduler::new(chrono_tz::UTC);
    let TestRuntime { manager, .. } = test_runtime(
        temporary.path(),
        Vec::new(),
        MemorySessionStore::default(),
        scheduler.clone(),
    );

    let error = manager
        .wait_for_agent(&AgentRef::parse("official/ora-space.echo").expect("agent identity"))
        .await
        .expect_err("an agent nothing supplies is never ready");

    assert_eq!(
        error.public_error(),
        &PublicError::AgentRuntimeUnavailable(EmptyErrorParams {}),
    );
    scheduler.shutdown().await;
}
