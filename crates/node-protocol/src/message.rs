mod execution;
mod repository;
mod runtime_control;
mod session;
mod validation;
mod worktree;

pub use execution::{
    EventAck, EventAckMessage, ExecutionState, ExecutionStatus, ExecutionStatusMessage,
    GetExecutionStatus, GetExecutionStatusMessage,
};
pub use repository::{CloneRepository, CloneRepositoryMessage, CloneResultMessage};
pub use runtime_control::{ControlledClone, RuntimeBinding, RuntimeControlState};
use serde::{Deserialize, Serialize};
pub use session::{
    ControllerHeartbeat, ControllerHeartbeatMessage, Heartbeat, HeartbeatMessage, Hello,
    HelloAccepted, HelloAcceptedMessage, HelloMessage, NodeCapability,
};
pub use validation::MessageValidationError;
pub use validation::ValidateMessage;
pub use worktree::{
    EnsureWorktree, EnsureWorktreeMessage, RemoveWorktree, RemoveWorktreeMessage,
    WorktreeFailedMessage, WorktreeReadyMessage, WorktreeRemovalFailedMessage,
    WorktreeRemovedMessage,
};

/// Messages sent by the Controller; each business owns its envelope invariants.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "message_type", rename_all = "snake_case")]
pub enum ControllerToNodeMessage {
    Hello(HelloMessage),
    /// Sent on idle query ticks; the wire tag is shared with the Node heartbeat but directions
    /// decode through separate enums, so the two never mix.
    Heartbeat(ControllerHeartbeatMessage),
    CloneRepository(CloneRepositoryMessage),
    BindRuntime(RuntimeBinding),
    ControlledClone(ControlledClone),
    EnsureWorktree(EnsureWorktreeMessage),
    RemoveWorktree(RemoveWorktreeMessage),
    GetExecutionStatus(GetExecutionStatusMessage),
    EventAck(EventAckMessage),
}

impl ValidateMessage for ControllerToNodeMessage {
    /// Dispatches validation without introducing business rules into the codec.
    fn validate(&self) -> Result<(), MessageValidationError> {
        match self {
            Self::Hello(message) => message.validate(),
            Self::Heartbeat(message) => message.validate(),
            Self::CloneRepository(message) => message.validate(),
            Self::BindRuntime(message) => message.validate(),
            Self::ControlledClone(message) => message.validate(),
            Self::EnsureWorktree(message) => message.validate(),
            Self::RemoveWorktree(message) => message.validate(),
            Self::GetExecutionStatus(message) => message.validate(),
            Self::EventAck(message) => message.validate(),
        }
    }
}

/// Messages sent by the Node; each business owns its envelope invariants.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "message_type", rename_all = "snake_case")]
pub enum NodeToControllerMessage {
    CloneResult(CloneResultMessage),
    HelloAccepted(HelloAcceptedMessage),
    Heartbeat(HeartbeatMessage),
    ExecutionStatus(ExecutionStatusMessage),
    RuntimeControlState(RuntimeControlState),
    WorktreeReady(WorktreeReadyMessage),
    WorktreeFailed(WorktreeFailedMessage),
    WorktreeRemoved(WorktreeRemovedMessage),
    WorktreeRemovalFailed(WorktreeRemovalFailedMessage),
}

impl ValidateMessage for NodeToControllerMessage {
    /// Dispatches validation without introducing business rules into the codec.
    fn validate(&self) -> Result<(), MessageValidationError> {
        match self {
            Self::CloneResult(message) => message.validate(),
            Self::HelloAccepted(message) => message.validate(),
            Self::Heartbeat(message) => message.validate(),
            Self::ExecutionStatus(message) => message.validate(),
            Self::RuntimeControlState(message) => message.validate(),
            Self::WorktreeReady(message) => message.validate(),
            Self::WorktreeFailed(message) => message.validate(),
            Self::WorktreeRemoved(message) => message.validate(),
            Self::WorktreeRemovalFailed(message) => message.validate(),
        }
    }
}
