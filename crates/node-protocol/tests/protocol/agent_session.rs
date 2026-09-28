use super::session::node;
use super::support::*;
use ora_node_protocol::*;
use serde_json::{Map, Value, json};

const OPERATION: &str = "run-1";
const EXECUTION: &str = "execution-session";

/// Builds a single-text user turn.
fn turn(turn_id: &str, text: &str) -> UserTurn {
    UserTurn {
        turn_id: TurnId::new(turn_id),
        content: vec![ContentBlock::Text {
            text: text.to_owned(),
        }],
    }
}

/// A session start with its git identity and first turn, with independent JSON.
fn start() -> Case {
    Case {
        message: Message::Controller(ControllerToNodeMessage::StartAgentSession(
            StartAgentSessionMessage {
                protocol_version: CURRENT_PROTOCOL_VERSION,
                operation_id: OperationId::new(OPERATION),
                execution_id: ExecutionId::new(EXECUTION),
                payload: StartAgentSession {
                    spec: AgentSessionSpec {
                        node_id: NodeId::new("node-1"),
                        agent_plugin_id: PluginId::new("ora.1a2b/claude-code"),
                        agent_plugin_version: PluginVersion::new("1.2.0"),
                        checkout_execution_id: ExecutionId::new("execution-clone"),
                        git_identity: GitIdentity {
                            name: "Ada Lovelace".to_owned(),
                            email: "ada@example.com".to_owned(),
                        },
                        initial_turn: turn("turn-1", "Fix the failing login test."),
                    },
                },
            },
        )),
        wire: json!({
            "message_type": "start_agent_session",
            "protocol_version": 1,
            "operation_id": OPERATION,
            "execution_id": EXECUTION,
            "payload": {"spec": {
                "node_id": "node-1",
                "agent_plugin_id": "ora.1a2b/claude-code",
                "agent_plugin_version": "1.2.0",
                "checkout_execution_id": "execution-clone",
                "git_identity": {"name": "Ada Lovelace", "email": "ada@example.com"},
                "initial_turn": {"turn_id": "turn-1",
                    "content": [{"type": "text", "text": "Fix the failing login test."}]}
            }}
        }),
    }
}

/// One Thread event carrying an opaque history record that belongs to a turn.
fn thread_event() -> Case {
    let record = json!({"type": "agent_message", "text": "Looking at the test."});
    let Value::Object(record) = record else {
        panic!("fixture record must be an object")
    };
    Case {
        message: Message::Node(NodeToControllerMessage::ThreadEvent(ThreadEventMessage {
            protocol_version: CURRENT_PROTOCOL_VERSION,
            operation_id: OperationId::new(OPERATION),
            execution_id: ExecutionId::new(EXECUTION),
            sequence: Sequence::new(/*value*/ 3),
            payload: ThreadEvent {
                turn_id: Some(TurnId::new("turn-1")),
                record,
                truncated: false,
            },
        })),
        wire: json!({"message_type": "thread_event", "protocol_version": 1,
            "operation_id": OPERATION, "execution_id": EXECUTION, "sequence": 3,
            "payload": {"turn_id": "turn-1",
                "record": {"type": "agent_message", "text": "Looking at the test."}}}),
    }
}

/// The start command has a fixed wire shape and the shared envelope guarantees.
#[tokio::test]
async fn start_agent_session_round_trips() -> Result<(), TestError> {
    let case = start();
    case.assert_wire().await?;
    case.assert_round_trip().await?;
    case.assert_envelope_rejections().await?;
    case.assert_fields(
        &[
            "/payload/spec/agent_plugin_id",
            "/payload/spec/checkout_execution_id",
            "/payload/spec/git_identity",
            "/payload/spec/initial_turn",
        ],
        &[
            ("/operation_id", "operation_id"),
            ("/execution_id", "execution_id"),
            ("/payload/spec/node_id", "node_id"),
            (
                "/payload/spec/checkout_execution_id",
                "checkout_execution_id",
            ),
            ("/payload/spec/initial_turn/turn_id", "turn_id"),
        ],
    )
    .await
}

/// Identity values that would corrupt a Git signature line and unbounded turns never cross.
#[tokio::test]
async fn start_agent_session_rejects_unsafe_identity_and_turns() -> Result<(), TestError> {
    let oversized = "x".repeat(MAX_USER_TURN_TEXT_BYTES + 1);
    let cases = [
        (
            "/payload/spec/git_identity/name",
            json!("Ada\nLovelace"),
            MessageValidationError::InvalidGitIdentity,
        ),
        (
            "/payload/spec/git_identity/name",
            json!("Ada <ada@example.com>"),
            MessageValidationError::InvalidGitIdentity,
        ),
        (
            "/payload/spec/git_identity/email",
            json!("ada example.com"),
            MessageValidationError::InvalidGitIdentity,
        ),
        (
            "/payload/spec/initial_turn/content",
            json!([]),
            MessageValidationError::InvalidUserTurn,
        ),
        (
            "/payload/spec/initial_turn/content/0/text",
            json!(oversized),
            MessageValidationError::InvalidUserTurn,
        ),
        (
            "/payload/spec/agent_plugin_id",
            json!("claude-code"),
            MessageValidationError::InvalidPluginId,
        ),
    ];
    for (path, value, expected) in cases {
        let mut wire = start().wire;
        replace(&mut wire, path, value);
        reject_semantics(Peer::Controller, &wire, path, expected).await?;
    }
    Ok(())
}

/// Thread events are ordinary sequenced events whose record stays opaque and bounded.
#[tokio::test]
async fn thread_events_round_trip_and_bound_the_record() -> Result<(), TestError> {
    let case = thread_event();
    case.assert_wire().await?;
    case.assert_round_trip().await?;
    case.assert_envelope_rejections().await?;
    case.assert_fields(
        &["/sequence", "/payload/record"],
        &[
            ("/operation_id", "operation_id"),
            ("/execution_id", "execution_id"),
            ("/payload/turn_id", "turn_id"),
        ],
    )
    .await?;

    // A truncated record without a turn keeps the optional fields explicit on the wire.
    let mut truncated = thread_event();
    let Message::Node(NodeToControllerMessage::ThreadEvent(message)) = &mut truncated.message
    else {
        panic!("expected thread event")
    };
    message.payload.turn_id = None;
    message.payload.truncated = true;
    let payload = truncated.wire["payload"]
        .as_object_mut()
        .unwrap_or_else(|| panic!("payload must be an object"));
    payload.remove("turn_id");
    payload.insert("truncated".to_owned(), json!(true));
    truncated.assert_wire().await?;

    let mut wire = thread_event().wire;
    replace(&mut wire, "/payload/record", json!(["not", "an", "object"]));
    reject_structure(Peer::Node, &wire, "record must be an object").await;
    let mut huge = Map::new();
    huge.insert(
        "text".to_owned(),
        json!("x".repeat(MAX_THREAD_RECORD_BYTES)),
    );
    replace(&mut wire, "/payload/record", Value::Object(huge));
    reject_semantics(
        Peer::Node,
        &wire,
        "oversized record",
        MessageValidationError::ThreadRecordTooLarge,
    )
    .await
}

/// The terminal event and a Completed status decode as the session family.
#[tokio::test]
async fn session_end_round_trips_as_event_and_completed_status() -> Result<(), TestError> {
    let result = AgentSessionResult::AgentSessionEnded(AgentSessionEnded {
        node: node(),
        reason: AgentSessionEndReason::AgentFailed,
        detail: Some("agent_plugin_unavailable".to_owned()),
    });
    let result_wire = json!({"kind": "agent_session_ended", "result": {
        "node": {"node_id": "node-1", "incarnation_id": "incarnation-1"},
        "reason": "agent_failed", "detail": "agent_plugin_unavailable"}});
    let event = Case {
        message: Message::Node(NodeToControllerMessage::AgentSessionEnded(
            AgentSessionEndedMessage {
                protocol_version: CURRENT_PROTOCOL_VERSION,
                operation_id: OperationId::new(OPERATION),
                execution_id: ExecutionId::new(EXECUTION),
                sequence: Sequence::new(/*value*/ 9),
                payload: result.clone(),
            },
        )),
        wire: json!({"message_type": "agent_session_ended", "protocol_version": 1,
            "operation_id": OPERATION, "execution_id": EXECUTION, "sequence": 9,
            "payload": result_wire}),
    };
    let status = Case {
        message: Message::Node(NodeToControllerMessage::ExecutionStatus(
            ExecutionStatusMessage {
                protocol_version: CURRENT_PROTOCOL_VERSION,
                operation_id: OperationId::new(OPERATION),
                execution_id: ExecutionId::new(EXECUTION),
                payload: ExecutionStatus {
                    node: node(),
                    state: ExecutionState::Completed(ExecutionResult::AgentSession(result)),
                },
            },
        )),
        wire: json!({"message_type": "execution_status", "protocol_version": 1,
            "operation_id": OPERATION, "execution_id": EXECUTION,
            "payload": {"node": {"node_id": "node-1", "incarnation_id": "incarnation-1"},
                "state": {"state": "completed", "result": result_wire}}}),
    };
    for case in [&event, &status] {
        case.assert_wire().await?;
        case.assert_round_trip().await?;
        case.assert_envelope_rejections().await?;
    }
    status.assert_historical_node().await?;
    let mut wire = event.wire;
    replace(
        &mut wire,
        "/payload/result/detail",
        json!("Error: agent crashed"),
    );
    reject_semantics(
        Peer::Node,
        &wire,
        "free-form detail",
        MessageValidationError::InvalidDetailCode,
    )
    .await
}

/// Session commands and their replies carry command identity but no event sequence.
#[tokio::test]
async fn session_commands_and_replies_round_trip() -> Result<(), TestError> {
    let envelope = |payload: Value, message_type: &str| {
        json!({"message_type": message_type, "protocol_version": 1,
            "operation_id": OPERATION, "execution_id": EXECUTION, "payload": payload})
    };
    let cases = [
        Case {
            message: Message::Controller(ControllerToNodeMessage::SubmitUserTurn(
                SubmitUserTurnMessage {
                    protocol_version: CURRENT_PROTOCOL_VERSION,
                    operation_id: OperationId::new(OPERATION),
                    execution_id: ExecutionId::new(EXECUTION),
                    payload: SubmitUserTurn {
                        node_id: NodeId::new("node-1"),
                        command_id: CommandId::new("command-1"),
                        turn: turn("turn-2", "Also update the changelog."),
                    },
                },
            )),
            wire: envelope(
                json!({"node_id": "node-1", "command_id": "command-1", "turn": {"turn_id": "turn-2",
                    "content": [{"type": "text", "text": "Also update the changelog."}]}}),
                "submit_user_turn",
            ),
        },
        Case {
            message: Message::Controller(ControllerToNodeMessage::EndSession(EndSessionMessage {
                protocol_version: CURRENT_PROTOCOL_VERSION,
                operation_id: OperationId::new(OPERATION),
                execution_id: ExecutionId::new(EXECUTION),
                payload: EndSession {
                    node_id: NodeId::new("node-1"),
                    command_id: CommandId::new("command-2"),
                    reason: EndSessionReason::IdleTimeout,
                },
            })),
            wire: envelope(
                json!({"node_id": "node-1", "command_id": "command-2", "reason": "idle_timeout"}),
                "end_session",
            ),
        },
        Case {
            message: Message::Node(NodeToControllerMessage::SessionCommandAccepted(
                SessionCommandAcceptedMessage {
                    protocol_version: CURRENT_PROTOCOL_VERSION,
                    operation_id: OperationId::new(OPERATION),
                    execution_id: ExecutionId::new(EXECUTION),
                    payload: SessionCommandAccepted {
                        command_id: CommandId::new("command-1"),
                    },
                },
            )),
            wire: envelope(
                json!({"command_id": "command-1"}),
                "session_command_accepted",
            ),
        },
        Case {
            message: Message::Node(NodeToControllerMessage::SessionCommandRejected(
                SessionCommandRejectedMessage {
                    protocol_version: CURRENT_PROTOCOL_VERSION,
                    operation_id: OperationId::new(OPERATION),
                    execution_id: ExecutionId::new(EXECUTION),
                    payload: SessionCommandRejected {
                        command_id: CommandId::new("command-2"),
                        reason: SessionCommandRejection::SessionEnded,
                    },
                },
            )),
            wire: envelope(
                json!({"command_id": "command-2", "reason": "session_ended"}),
                "session_command_rejected",
            ),
        },
    ];
    for case in &cases {
        case.assert_wire().await?;
        case.assert_round_trip().await?;
        case.assert_envelope_rejections().await?;
        case.assert_fields(
            &["/payload/command_id"],
            &[
                ("/operation_id", "operation_id"),
                ("/execution_id", "execution_id"),
                ("/payload/command_id", "command_id"),
            ],
        )
        .await?;
    }
    Ok(())
}
