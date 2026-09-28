//! Plugin install and removal commands and their terminal event.

use super::validation::{validate_execution_ids, validate_protocol_version};
use crate::{
    ExecutionId, InstallPluginsSpec, MessageValidationError, OperationId, PluginExecutionResult,
    ProtocolVersion, RemovePluginsSpec, Sequence, ValidateMessage,
};
use serde::{Deserialize, Serialize};

/// Installs Cloud's planned plugin set without starting any plugin.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InstallPlugins {
    pub spec: InstallPluginsSpec,
}

/// Removes Cloud's planned plugin set.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RemovePlugins {
    pub spec: RemovePluginsSpec,
}

/// Correlates an install with its operation and durable execution identity.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct InstallPluginsMessage {
    pub protocol_version: ProtocolVersion,
    pub operation_id: OperationId,
    pub execution_id: ExecutionId,
    pub payload: InstallPlugins,
}

/// Correlates a removal with its operation and durable execution identity.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemovePluginsMessage {
    pub protocol_version: ProtocolVersion,
    pub operation_id: OperationId,
    pub execution_id: ExecutionId,
    pub payload: RemovePlugins,
}

/// One retained terminal plugin event; status queries reuse its payload without acknowledging it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PluginsResultMessage {
    pub protocol_version: ProtocolVersion,
    pub operation_id: OperationId,
    pub execution_id: ExecutionId,
    pub sequence: Sequence,
    pub payload: PluginExecutionResult,
}

impl ValidateMessage for InstallPluginsMessage {
    /// Rejects unverifiable payloads before the codec writes any bytes.
    fn validate(&self) -> Result<(), MessageValidationError> {
        validate_protocol_version(self.protocol_version)?;
        validate_execution_ids(&self.operation_id, &self.execution_id)?;
        self.payload.spec.validate()
    }
}

impl ValidateMessage for RemovePluginsMessage {
    /// Rejects empty or ambiguous removal sets before the codec writes any bytes.
    fn validate(&self) -> Result<(), MessageValidationError> {
        validate_protocol_version(self.protocol_version)?;
        validate_execution_ids(&self.operation_id, &self.execution_id)?;
        self.payload.spec.validate()
    }
}

impl ValidateMessage for PluginsResultMessage {
    /// Applies plugin-owned terminal checks before event delivery or replay.
    fn validate(&self) -> Result<(), MessageValidationError> {
        validate_protocol_version(self.protocol_version)?;
        validate_execution_ids(&self.operation_id, &self.execution_id)?;
        self.payload.validate()
    }
}
