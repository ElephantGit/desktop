#![allow(clippy::unwrap_used)]
mod server;
use super::*;
use pretty_assertions::assert_eq;
use server::Fixture;

/// Uses the Node-local operation identity, deliberately distinct from the Cloud run identity.
fn preview() -> ThreadPreviewMessage {
    ThreadPreviewMessage {
        protocol_version: CURRENT_PROTOCOL_VERSION,
        operation_id: OperationId::new("node-operation"),
        execution_id: ExecutionId::new("execution"),
        payload: ThreadPreview {
            version: 7,
            blocks: vec![ThreadPreviewBlock {
                history_sequence: 42,
                turn_id: Some(TurnId::new("turn")),
                record: serde_json::from_value(serde_json::json!({"text":"正文"})).unwrap(),
            }],
            incomplete: true,
        },
    }
}

/// Real gRPC transport must preserve the snapshot, holder, lease and Cloud-owned run identity.
#[tokio::test]
async fn relays_snapshot_and_empty_clear_without_receipts() {
    let fixture = Fixture::new().await;
    assert!(fixture.store.supports_thread_previews());
    let mut message = preview();
    fixture
        .store
        .publish_thread_preview(&fixture.node, &message)
        .await
        .unwrap();
    assert_eq!(
        fixture.server.previews(),
        vec![proto::PublishThreadPreviewRequest {
            epoch: 17,
            operation_id: "cloud-run".into(),
            execution_id: "execution".into(),
            version: 7,
            blocks: vec![proto::ThreadPreviewBlock {
                history_sequence: 42,
                turn_id: "turn".into(),
                record: "{\"text\":\"正文\"}".into(),
            }],
            incomplete: true,
        }]
    );
    message.payload.version = 8;
    message.payload.blocks.clear();
    fixture
        .store
        .publish_thread_preview(&fixture.node, &message)
        .await
        .unwrap();
    assert!(fixture.server.previews()[1].blocks.is_empty());
    assert!(fixture.server.events().is_empty());
}

/// Foreign envelopes, invalid snapshots and ended executions never reach the preview RPC.
#[tokio::test]
async fn rejects_invalid_foreign_and_terminal_previews() {
    let fixture = Fixture::new().await;
    let mut message = preview();
    message.payload.version = 0;
    assert!(
        fixture
            .store
            .publish_thread_preview(&fixture.node, &message)
            .await
            .is_err()
    );
    message = preview();
    message.operation_id = OperationId::new("foreign");
    assert!(
        fixture
            .store
            .publish_thread_preview(&fixture.node, &message)
            .await
            .is_err()
    );
    let foreign = NodeRuntimeIdentity {
        node_id: NodeId::new("foreign"),
        ..fixture.node.clone()
    };
    assert!(
        fixture
            .store
            .publish_thread_preview(&foreign, &preview())
            .await
            .is_err()
    );
    fixture.server.end();
    assert!(
        fixture
            .store
            .publish_thread_preview(&fixture.node, &preview())
            .await
            .is_err()
    );
    assert!(fixture.server.previews().is_empty());
}

/// Ephemeral failures are not durably retried; only a stale-lease verdict drops the shared lease.
#[tokio::test]
async fn preview_failure_is_single_attempt_and_stale_lease_fences_following_calls() {
    let fixture = Fixture::new().await;
    fixture.server.fail(tonic::Code::Unknown);
    assert!(
        fixture
            .store
            .publish_thread_preview(&fixture.node, &preview())
            .await
            .is_err()
    );
    assert_eq!(fixture.server.previews().len(), 1);
    assert_eq!(fixture.store.epoch().unwrap(), 17);
    fixture.store.set_lease(Some(18));
    fixture.server.fail(tonic::Code::FailedPrecondition);
    assert!(
        fixture
            .store
            .publish_thread_preview(&fixture.node, &preview())
            .await
            .is_err()
    );
    assert_eq!(fixture.server.previews()[1].epoch, 18);
    assert!(fixture.store.epoch().is_err());
    assert!(
        fixture
            .store
            .publish_thread_preview(&fixture.node, &preview())
            .await
            .is_err()
    );
    assert_eq!(fixture.server.previews().len(), 2);
}

/// Settled body projections and stable positions survive the actual adapter and protobuf wire.
#[tokio::test]
async fn settled_projection_and_legacy_absence_are_preserved() {
    let fixture = Fixture::new().await;
    let first = ThreadEventMessage {
        protocol_version: CURRENT_PROTOCOL_VERSION,
        operation_id: OperationId::new("node-operation"),
        execution_id: ExecutionId::new("execution"),
        sequence: Sequence::new(1),
        payload: ThreadEvent {
            turn_id: Some(TurnId::new("turn")),
            record: serde_json::from_value(serde_json::json!({"text":"正文"})).unwrap(),
            truncated: false,
            visible_text: Some("正文".into()),
            visible_text_truncated: true,
            history_sequence: Some(42),
        },
    };
    let mut legacy = first.clone();
    legacy.sequence = Sequence::new(2);
    legacy.payload.visible_text = None;
    legacy.payload.visible_text_truncated = false;
    legacy.payload.history_sequence = None;
    fixture
        .store
        .take_over_thread(&fixture.node, &[first, legacy])
        .await
        .unwrap();
    let expected = proto::ThreadEvent {
        sequence: 1,
        turn_id: Some("turn".into()),
        record: "{\"text\":\"正文\"}".into(),
        truncated: false,
        visible_text: Some("正文".into()),
        visible_text_truncated: true,
        history_sequence: Some(42),
    };
    assert_eq!(
        fixture.server.events()[0].events,
        vec![
            expected.clone(),
            proto::ThreadEvent {
                sequence: 2,
                visible_text: None,
                visible_text_truncated: false,
                history_sequence: None,
                ..expected
            }
        ]
    );
}
