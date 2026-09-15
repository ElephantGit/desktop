//! Host-owned Plugin log level of one installed plugin identity.
//!
//! The level is the lowest severity the host persists into that plugin's own log file. It is
//! independent of the Ora runtime log level and of every other plugin, and it is never visible
//! to the plugin process itself — the host filters on the plugin's behalf.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::RuntimeLogLevel;

/// Reads the effective Plugin log level of one plugin identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "plugin-log-level.ts")]
pub struct GetPluginLogLevelRequest {
    pub plugin_id: String,
}

/// Persists and immediately applies a new Plugin log level to one plugin identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "plugin-log-level.ts")]
pub struct SetPluginLogLevelRequest {
    pub plugin_id: String,
    pub level: RuntimeLogLevel,
}

/// The effective level and whether it comes from an explicit setting rather than the default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "plugin-log-level.ts")]
pub struct PluginLogLevelResponse {
    pub plugin_id: String,
    pub level: RuntimeLogLevel,
    pub configured: bool,
}

/// Exports the Plugin log level DTO family into its own TypeScript module.
pub(super) fn export(config: &ts_rs::Config) -> Result<(), ts_rs::ExportError> {
    GetPluginLogLevelRequest::export(config)?;
    SetPluginLogLevelRequest::export(config)?;
    PluginLogLevelResponse::export(config)?;
    Ok(())
}
