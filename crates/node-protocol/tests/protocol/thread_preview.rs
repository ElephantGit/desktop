use super::support::*;
use ora_node_protocol::*;
use pretty_assertions::assert_eq;
use serde_json::json;

/// Older greetings omit the flag; opted-in peers retain it through the real framed codec.
#[tokio::test]
async fn preview_handshake_is_optional_and_requires_agent_sessions() -> Result<(), TestError> {
    let wire = json!({"message_type":"hello_accepted","protocol_version":1,"payload":{"selected_version":1,"thread_previews":true,"node":{"node_id":"node","incarnation_id":"incarnation"},"capabilities":["agent_session"]}});
    let accepted: NodeToControllerMessage = serde_json::from_value(wire.clone())?;
    assert_eq!(round_trip_node(accepted.clone()).await?, accepted);
    let mut unsupported = wire;
    unsupported["payload"]["capabilities"] = json!(["repository_clone"]);
    reject_semantics(
        Peer::Node,
        &unsupported,
        "preview without Agent capability",
        MessageValidationError::InvalidThreadPreview,
    )
    .await?;
    Ok(())
}

/// A preview carries no durable event sequence, so the receiver cannot manufacture an Ack.
#[tokio::test]
async fn preview_round_trips_without_a_durable_event_sequence() -> Result<(), TestError> {
    let wire = json!({"message_type":"thread_preview","protocol_version":1,"operation_id":"run","execution_id":"execution","payload":{"version":1,"incomplete":false,"blocks":[{"history_sequence":7,"turn_id":"turn","record":{"type":"update"}}]}});
    let message: NodeToControllerMessage = serde_json::from_value(wire.clone())?;
    assert_eq!(round_trip_node(message.clone()).await?, message);
    assert_eq!(serde_json::to_value(message)?, wire);
    Ok(())
}

/// Receiving and sending both reject duplicate positions and content over the bounded budget.
#[tokio::test]
async fn preview_rejects_unbounded_or_ambiguous_snapshots() -> Result<(), TestError> {
    let base = json!({"message_type":"thread_preview","protocol_version":1,"operation_id":"run","execution_id":"execution","payload":{"version":1,"incomplete":false,"blocks":[]}});
    let mut duplicate = base.clone();
    duplicate["payload"]["blocks"] = json!([{"history_sequence":7,"turn_id":null,"record":{}},{"history_sequence":7,"turn_id":null,"record":{}}]);
    reject_semantics(
        Peer::Node,
        &duplicate,
        "duplicate pending position",
        MessageValidationError::InvalidThreadPreview,
    )
    .await?;
    let mut oversized = base;
    oversized["payload"]["blocks"] =
        json!([{"history_sequence":1,"turn_id":null,"record":{"text":"x".repeat(65537)}}]);
    reject_semantics(
        Peer::Node,
        &oversized,
        "oversized preview record",
        MessageValidationError::InvalidThreadPreview,
    )
    .await?;
    Ok(())
}
