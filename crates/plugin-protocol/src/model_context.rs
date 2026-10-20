//! Versioned model adapter startup. Only this live RPC carries the temporary token.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Non-secret admission identity fixed before an execution is queued.
#[derive(Debug, Clone, Deserialize, Serialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export_to = "model_context.ts")]
pub struct AgentModelConfiguration {
    pub plugin_id: String,
    pub plugin_version: String,
    pub target: String,
    pub package_sha256: String,
    pub schema_version: u32,
    pub declaration_fingerprint: String,
    pub adapter_version: u32,
    #[ts(type = "number")]
    pub configuration_revision: u64,
    /// Validated scalar JSON retained as text so arbitrary numeric literals survive JavaScript.
    pub settings_json: String,
    pub protocol: AgentModelProtocol,
    pub model: AgentBoundModel,
}

/// Exactly one protocol accepted by the versioned adapter contract.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, TS, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
#[ts(export_to = "model_context.ts")]
pub enum AgentModelProtocol {
    OpenaiCompletions,
    AnthropicMessages,
}

impl AgentModelProtocol {
    /// Returns the stable protocol spelling used in host capability reports.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenaiCompletions => "openai-completions",
            Self::AnthropicMessages => "anthropic-messages",
        }
    }
}

/// Model metadata from the authenticated frozen binding, never upstream discovery.
#[derive(Debug, Clone, Deserialize, Serialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export_to = "model_context.ts")]
pub struct AgentBoundModel {
    pub id: String,
    pub name: String,
    #[ts(type = "number")]
    pub context_window: u64,
    #[ts(type = "number")]
    pub max_tokens: u64,
}

/// Sensitive in-memory RPC input; hosts must never persist it or include it in diagnostics.
#[derive(Clone, Deserialize, Serialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export_to = "model_context.ts")]
pub struct AgentModelContext {
    pub configuration: AgentModelConfiguration,
    pub proxy_base_url: String,
    pub token: String,
}

impl std::fmt::Debug for AgentModelContext {
    /// Neither settings nor tokens are safe diagnostic payloads.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AgentModelContext([redacted])")
    }
}

/// Adapter proof that this exact startup configuration was applied before ACP became ready.
#[derive(Debug, Clone, Deserialize, Serialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export_to = "model_context.ts")]
pub struct AgentModelApplied {
    pub adapter_version: u32,
    pub declaration_fingerprint: String,
    #[ts(type = "number")]
    pub configuration_revision: u64,
}

/// Exports the sensitive startup seam from the same Rust declarations the host consumes.
pub(crate) fn export(config: &ts_rs::Config) -> Result<(), ts_rs::ExportError> {
    AgentModelConfiguration::export(config)?;
    AgentModelProtocol::export(config)?;
    AgentBoundModel::export(config)?;
    AgentModelContext::export(config)?;
    AgentModelApplied::export(config)
}
