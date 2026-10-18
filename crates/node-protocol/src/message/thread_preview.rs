//! Optional, memory-only replacement snapshots; never part of execution event acknowledgement.
use super::validation::{
    MessageValidationError, ValidateMessage, validate_execution_ids, validate_protocol_version,
};
use crate::{ExecutionId, OperationId, ProtocolVersion, TurnId};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// One pending history position, replaced rather than appended on every update.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ThreadPreviewBlock {
    pub history_sequence: u32,
    pub turn_id: Option<TurnId>,
    pub record: Map<String, Value>,
}

/// A bounded complete snapshot of one execution's still-open records.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ThreadPreview {
    pub version: u64,
    pub blocks: Vec<ThreadPreviewBlock>,
    pub incomplete: bool,
}

/// No sequence is present: this message cannot be persisted or acknowledged as execution evidence.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ThreadPreviewMessage {
    pub protocol_version: ProtocolVersion,
    pub operation_id: OperationId,
    pub execution_id: ExecutionId,
    pub payload: ThreadPreview,
}

impl ValidateMessage for ThreadPreviewMessage {
    /// Bounds bytes and identities independently of the durable event contract.
    fn validate(&self) -> Result<(), MessageValidationError> {
        validate_protocol_version(self.protocol_version)?;
        validate_execution_ids(&self.operation_id, &self.execution_id)?;
        let mut positions = std::collections::HashSet::new();
        if self.payload.version == 0
            || self.payload.version > i64::MAX as u64
            || self.payload.blocks.len() > 32
            || self.payload.blocks.iter().any(|block| {
                !positions.insert(block.history_sequence)
                    || block
                        .turn_id
                        .as_ref()
                        .is_some_and(|turn| turn.as_str().is_empty() || turn.as_str().len() > 200)
                    || serde_json::to_vec(&block.record).map_or(true, |bytes| bytes.len() > 65536)
            })
            || serde_json::to_vec(&self.payload).map_or(true, |bytes| bytes.len() > 256 * 1024)
        {
            return Err(MessageValidationError::InvalidThreadPreview);
        }
        Ok(())
    }
}
