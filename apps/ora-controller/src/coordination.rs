use crate::*;

/// Takes one Node message through the store boundary. Only an actually received event can produce
/// an Ack, and only after the store confirmed the takeover; queries never acknowledge anything.
pub async fn take_over<S: CoordinationStore>(
    store: &S,
    session: &NodeRuntimeIdentity,
    message: &NodeToControllerMessage,
) -> Result<Option<EventAckMessage>, Error> {
    message.validate()?;
    match message {
        NodeToControllerMessage::RuntimeControlState(state) => {
            if state.binding.node_id != session.node_id.as_str()
                || state.binding.node_incarnation_id != session.incarnation_id.as_str()
            {
                return Err(Error::Conflict);
            }
            store.acknowledge_runtime_binding(state).await?;
            Ok(None)
        }
        NodeToControllerMessage::PluginsResult(event) => {
            store.take_over_plugins(session, event).await?;
            Ok(Some(EventAckMessage { protocol_version: CURRENT_PROTOCOL_VERSION,
                operation_id: event.operation_id.clone(), execution_id: event.execution_id.clone(), sequence: event.sequence,
                payload: EventAck { node_id: session.node_id.clone() },
            }))
        }
        NodeToControllerMessage::CloneResult(event) => {
            store.take_over_node_event(session, event).await?;
            Ok(Some(EventAckMessage {
                protocol_version: CURRENT_PROTOCOL_VERSION,
                operation_id: event.operation_id.clone(),
                execution_id: event.execution_id.clone(),
                sequence: event.sequence,
                payload: EventAck {
                    node_id: session.node_id.clone(),
                },
            }))
        }
        NodeToControllerMessage::ExecutionStatus(status) => {
            if status.payload.node != *session {
                return Err(Error::Conflict);
            }
            match &status.payload.state {
                ExecutionState::Completed(ExecutionResult::Clone(result)) => {
                    store
                        .record_queried_result(
                            session,
                            &status.operation_id,
                            &status.execution_id,
                            result,
                        )
                        .await?;
                }
                ExecutionState::Completed(ExecutionResult::Plugin(result)) => {
                    store.record_queried_plugins(session, &status.operation_id, &status.execution_id, result).await?;
                }
                // This Controller dispatches clones and plugins; any other result family cannot
                // belong to one of its dispatches.
                ExecutionState::Completed(
                    ExecutionResult::Worktree(_)
                    | ExecutionResult::AgentSession(_)
                    | ExecutionResult::Revision(_),
                ) => {
                    return Err(Error::Conflict);
                }
                // A status for an unknown dispatch is a conflict even when it carries no result.
                ExecutionState::Unknown | ExecutionState::Accepted | ExecutionState::Running => {
                    if store.original_plugin_dispatch(session, &status.operation_id, &status.execution_id).await?.is_none() {
                        store.original_dispatch(session, &status.operation_id, &status.execution_id).await?;
                    }
                }
            }
            Ok(None)
        }
        NodeToControllerMessage::Heartbeat(heartbeat) if heartbeat.payload.node == *session => {
            Ok(None)
        }
        NodeToControllerMessage::Heartbeat(_)
        | NodeToControllerMessage::HelloAccepted(_)
        | NodeToControllerMessage::WorktreeReady(_)
        | NodeToControllerMessage::WorktreeFailed(_)
        | NodeToControllerMessage::WorktreeRemoved(_)
        | NodeToControllerMessage::WorktreeRemovalFailed(_)
        // Session and delivery executions are never dispatched by this Controller yet,
        // so their events and replies cannot match a dispatch it owns.
        | NodeToControllerMessage::ThreadEvent(_)
        | NodeToControllerMessage::AgentSessionEnded(_)
        | NodeToControllerMessage::SessionCommandAccepted(_)
        | NodeToControllerMessage::SessionCommandRejected(_)
        | NodeToControllerMessage::RevisionResult(_)
        | NodeToControllerMessage::UploadGrantNeeded(_) => Err(Error::Conflict),
    }
}
