//! Non-secret inputs and host facts of the plugin-owned model adapter path.
//!
//! Only frozen identity and validated Settings cross the control protocol. The temporary token
//! that authorizes model calls is obtained by the Node over its own mTLS identity and never
//! appears in a command, the ledger or a status report.

use crate::{MessageValidationError, PluginId, PluginVersion, Sha256Digest};
use serde::{Deserialize, Serialize};

/// The only adapter contract this protocol version can describe.
pub const MODEL_ADAPTER_CONTRACT_VERSION: u32 = 1;

/// Largest frozen Settings document; declarations bound their fields far below this.
pub const MAX_AGENT_SETTINGS_BYTES: usize = 64 * 1024;

/// Model wire protocols a platform proxy serves to an adapter.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModelProtocol {
    OpenaiCompletions,
    AnthropicMessages,
}

impl ModelProtocol {
    /// Parses Cloud's stable protocol spelling; any other name is not a supported protocol.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "openai-completions" => Some(Self::OpenaiCompletions),
            "anthropic-messages" => Some(Self::AnthropicMessages),
            _ => None,
        }
    }

    /// The spelling Cloud and the plugin protocol use for this protocol.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenaiCompletions => "openai-completions",
            Self::AnthropicMessages => "anthropic-messages",
        }
    }
}

/// Model catalog metadata frozen in the binding at admission, never upstream discovery.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BoundModel {
    pub id: String,
    pub name: String,
    pub context_window: u64,
    pub max_tokens: u64,
}

/// The exact Agent configuration Cloud froze for one run.
///
/// A retry replays this value; the Node never re-reads a personal default.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenAgentConfiguration {
    pub plugin_id: PluginId,
    pub plugin_version: PluginVersion,
    pub target: String,
    pub package_sha256: Sha256Digest,
    pub schema_version: u32,
    pub declaration_fingerprint: Sha256Digest,
    pub adapter_version: u32,
    pub configuration_revision: u64,
    /// Validated Settings kept as JSON text so numeric lexemes survive every hop unchanged.
    pub settings_json: String,
    pub protocol: ModelProtocol,
    pub model: BoundModel,
}

impl FrozenAgentConfiguration {
    /// Rejects a configuration that does not describe the session's own plugin release.
    pub(crate) fn validate(
        &self,
        plugin_id: &PluginId,
        plugin_version: &PluginVersion,
    ) -> Result<(), MessageValidationError> {
        self.package_sha256.validate()?;
        self.declaration_fingerprint.validate()?;
        // The settings must be one JSON object: adapters read named fields, never a bare value.
        let settings_object = self.settings_json.len() <= MAX_AGENT_SETTINGS_BYTES
            && serde_json::from_str::<serde_json::Value>(&self.settings_json)
                .is_ok_and(|settings| settings.is_object());
        if self.plugin_id != *plugin_id
            || self.plugin_version != *plugin_version
            || self.target.is_empty()
            || self.schema_version == 0
            || self.adapter_version != MODEL_ADAPTER_CONTRACT_VERSION
            || self.configuration_revision == 0
            || !settings_object
            || self.model.id.is_empty()
            || self.model.name.is_empty()
            || self.model.context_window == 0
            || self.model.max_tokens == 0
        {
            return Err(MessageValidationError::InvalidAgentConfiguration);
        }
        Ok(())
    }
}

/// What a Node can run on the adapter path, advertised in its handshake.
///
/// It is a fact about the Node binary and its model-access configuration, independent of any
/// plugin's declaration; a Controller dispatches a configured session only on an exact match.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelAdapterCapability {
    pub contract_version: u32,
    pub target: String,
    pub protocols: Vec<ModelProtocol>,
}

impl ModelAdapterCapability {
    /// Requires a known contract, a target and a non-repeating protocol set.
    pub(crate) fn validate(&self) -> Result<(), MessageValidationError> {
        let mut protocols = self.protocols.clone();
        protocols.sort();
        protocols.dedup();
        if self.contract_version != MODEL_ADAPTER_CONTRACT_VERSION
            || self.target.is_empty()
            || self.protocols.is_empty()
            || protocols.len() != self.protocols.len()
        {
            return Err(MessageValidationError::InvalidModelAdapterCapability);
        }
        Ok(())
    }

    /// Whether a session frozen with `configuration` can run on this Node.
    pub fn admits(&self, configuration: &FrozenAgentConfiguration) -> bool {
        self.contract_version == configuration.adapter_version
            && self.target == configuration.target
            && self.protocols.contains(&configuration.protocol)
    }
}
