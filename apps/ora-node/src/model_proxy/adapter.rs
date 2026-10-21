//! The plugin-owned adapter route: the Node hands over a grant, never CLI configuration.
//!
//! The Node keeps what a plugin must not choose (the binding, the grant, the proxy endpoint, the
//! private home and the trusted CA) and the adapter alone turns that into its CLI's native
//! configuration. The token therefore travels only inside the live startup context; it is never
//! placed in the plugin's environment, where every process it spawns would inherit it.

use super::{Grant, ModelAccessError};
use ora_node_protocol::{FrozenAgentConfiguration, ModelProtocol};
use ora_plugin_protocol::{
    AgentBoundModel, AgentModelConfiguration, AgentModelContext, AgentModelProtocol,
};
use std::{collections::BTreeMap, path::Path};

/// Proves the grant serves exactly the frozen configuration, then builds the adapter's input.
///
/// A grant for another protocol or model means the binding and the frozen configuration
/// disagree; starting anyway would let the adapter send requests the configuration never
/// admitted, so the session fails before any plugin process exists.
pub(super) fn start(
    grant: &Grant,
    configuration: &FrozenAgentConfiguration,
    ca: &Path,
    node_home: &Path,
) -> Result<
    (
        BTreeMap<String, String>,
        tempfile::TempDir,
        AgentModelContext,
    ),
    ModelAccessError,
> {
    let model = &configuration.model;
    if grant.protocol != configuration.protocol
        || grant.model.id != model.id
        || grant.model.name != model.name
        || grant.model.context_window != model.context_window
        || grant.model.max_tokens != model.max_tokens
    {
        return Err(ModelAccessError("model_grant_mismatch"));
    }
    let home = super::isolated_home(node_home, "agent-")?;
    let environment = super::home_variables(home.path(), ca);
    let context = AgentModelContext {
        configuration: AgentModelConfiguration {
            plugin_id: configuration.plugin_id.as_str().to_string(),
            plugin_version: configuration.plugin_version.as_str().to_string(),
            target: configuration.target.clone(),
            package_sha256: configuration.package_sha256.as_str().to_string(),
            schema_version: configuration.schema_version,
            declaration_fingerprint: configuration.declaration_fingerprint.as_str().to_string(),
            adapter_version: configuration.adapter_version,
            configuration_revision: configuration.configuration_revision,
            settings_json: configuration.settings_json.clone(),
            protocol: match configuration.protocol {
                ModelProtocol::OpenaiCompletions => AgentModelProtocol::OpenaiCompletions,
                ModelProtocol::AnthropicMessages => AgentModelProtocol::AnthropicMessages,
            },
            model: AgentBoundModel {
                id: model.id.clone(),
                name: model.name.clone(),
                context_window: model.context_window,
                max_tokens: model.max_tokens,
            },
        },
        proxy_base_url: grant.proxy_base_url.clone(),
        token: grant.token.clone(),
    };
    Ok((environment, home, context))
}
