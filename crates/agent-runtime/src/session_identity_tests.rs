//! Covers the host-chosen session identity: a host that names sessions after its own durable
//! records must never be able to attach a new conversation to an existing one.

use crate::ErrorClassification;
use crate::host::SessionStore;
use crate::test_host::{MemorySessionStore, TestRuntime, test_runtime};
use ora_contracts::StartSessionRequest;
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
