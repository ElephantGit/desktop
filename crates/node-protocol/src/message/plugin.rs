//! Plugin install and removal commands and their terminal event.

use super::validation::{validate_execution_ids, validate_protocol_version};
use crate::{
    ExecutionId, InstallPluginsSpec, MessageValidationError, OperationId, PluginExecutionResult,
    ProtocolVersion, RemovePluginsSpec, Sequence, ValidateMessage,
};
use serde::{Deserialize, Serialize};

/// The immutable input of a plugin execution, shared by dispatch and durable recovery.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "command", rename_all = "snake_case")]
pub enum PluginCommand {
    Install(InstallPluginsMessage),
    Remove(RemovePluginsMessage),
}

impl PluginCommand {
    /// Returns the Node-local operation identity of this attempt.
    pub fn operation_id(&self) -> &OperationId {
        match self {
            Self::Install(m) => &m.operation_id,
            Self::Remove(m) => &m.operation_id,
        }
    }

    /// Returns the stable execution identity used for retries and acknowledgements.
    pub fn execution_id(&self) -> &ExecutionId {
        match self {
            Self::Install(m) => &m.execution_id,
            Self::Remove(m) => &m.execution_id,
        }
    }

    /// Returns the only Node authorized to accept the command.
    pub fn node_id(&self) -> &crate::NodeId {
        match self {
            Self::Install(m) => &m.payload.spec.node_id,
            Self::Remove(m) => &m.payload.spec.node_id,
        }
    }

    /// Checks that every planned item has exactly one result of the appropriate kind/version.
    pub fn accepts_result(&self, result: &PluginExecutionResult) -> bool {
        use crate::PluginItemOutcome;
        if result.node().node_id != *self.node_id() {
            return false;
        }
        let PluginExecutionResult::PluginsCompleted(completed) = result else {
            return true;
        };
        let planned: Vec<_> = match self {
            Self::Install(m) => m
                .payload
                .spec
                .plugins
                .iter()
                .map(|p| (&p.plugin_id, &p.version))
                .collect(),
            Self::Remove(m) => m
                .payload
                .spec
                .plugins
                .iter()
                .map(|p| (&p.plugin_id, &p.version))
                .collect(),
        };
        completed.items.len() == planned.len()
            && planned.iter().all(|(id, version)| {
                let matches: Vec<_> = completed
                    .items
                    .iter()
                    .filter(|item| &item.plugin_id == *id)
                    .collect();
                matches.len() == 1
                    && match &matches[0].outcome {
                        PluginItemOutcome::Installed { version: installed } => {
                            matches!(self, Self::Install(_)) && installed == *version
                        }
                        PluginItemOutcome::Removed {} => matches!(self, Self::Remove(_)),
                        PluginItemOutcome::Failed { .. } => true,
                    }
            })
    }
}

impl crate::ValidateMessage for PluginCommand {
    /// Applies the same validation to wire messages and recovered inputs.
    fn validate(&self) -> Result<(), MessageValidationError> {
        match self {
            Self::Install(m) => m.validate(),
            Self::Remove(m) => m.validate(),
        }
    }
}

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
