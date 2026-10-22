//! On-demand read-only binding projections; deliberately absent from public Session payloads.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Requests at most 32 distinct Ora sessions in one bounded diagnostic read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "session-diagnostics.ts")]
pub struct GetSessionBindingsRequest {
    pub session_ids: Vec<String>,
}

/// Preserves per-session failure isolation with one result per distinct requested id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "session-diagnostics.ts")]
pub struct GetSessionBindingsResponse {
    pub sessions: Vec<SessionBindings>,
}

/// Explains why a binding history cannot be treated as complete.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export_to = "session-diagnostics.ts")]
pub enum BindingHistoryState {
    Readable,
    Unavailable,
    Incomplete,
    Unsupported,
}

/// Identifies one continuous binding, including repeated use of the same provider id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "session-diagnostics.ts")]
pub struct SessionBindingSegment {
    /// Stable within this history; live is a current-row fallback, never a historical timestamp.
    pub key: String,
    pub agent_ref: String,
    pub agent_session_id: String,
    pub recorded_at: Option<String>,
    pub current: bool,
}

/// Returns known facts without conflating a missing session with an empty binding history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export_to = "session-diagnostics.ts")]
pub enum SessionBindings {
    Available {
        #[serde(rename = "sessionId")]
        session_id: String,
        history: BindingHistoryState,
        segments: Vec<SessionBindingSegment>,
    },
    NotFound {
        #[serde(rename = "sessionId")]
        session_id: String,
    },
    Retry {
        #[serde(rename = "sessionId")]
        session_id: String,
    },
    Unavailable {
        #[serde(rename = "sessionId")]
        session_id: String,
    },
}

/// Exports the dedicated diagnostic DTO family.
pub(crate) fn export(config: &ts_rs::Config) -> Result<(), ts_rs::ExportError> {
    GetSessionBindingsRequest::export(config)?;
    GetSessionBindingsResponse::export(config)?;
    BindingHistoryState::export(config)?;
    SessionBindingSegment::export(config)?;
    SessionBindings::export(config)?;
    Ok(())
}
