//! Recognizes the versioned structured-log envelope the Plugin SDK writes to stderr.
//!
//! stderr is a mixed byte stream: SDK records, third-party library output, and native code all
//! share it. The envelope prefix only identifies the *format*; it grants nothing. Anything that
//! does not parse and validate as a v1 payload is handed back as a rejection so the caller can
//! preserve the bytes through the raw path instead of dropping them.

use ora_logging::LogLevel;
use serde_json::{Map, Value};

/// Prefix of every SDK structured record; the trailing space separates it from the JSON body.
pub const PLUGIN_LOG_ENVELOPE_V1_PREFIX: &str = "@ora/plugin-log/v1 ";

/// Prefix shared by every envelope version, used to notice a version the host does not speak.
const PLUGIN_LOG_ENVELOPE_FAMILY: &str = "@ora/plugin-log/";

/// Upper bound on `target` and `method` so a payload cannot smuggle a record-sized identifier.
pub const MAX_IDENTIFIER_BYTES: usize = 256;

/// The validated content of one v1 envelope; every field is still an untrusted plugin statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuredPayload {
    pub level: LogLevel,
    pub message: String,
    pub target: Option<String>,
    pub method: Option<String>,
    pub context: Map<String, Value>,
    pub error: Option<Map<String, Value>>,
}

/// Why a logical record was not accepted as a v1 envelope.
///
/// `NotAnEnvelope` is the ordinary case (plain third-party output); the other variants are
/// bounded failure classes the host may count and name without copying the payload anywhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvelopeRejection {
    NotAnEnvelope,
    UnknownVersion,
    MalformedJson,
    InvalidPayload,
}

impl EnvelopeRejection {
    /// Names the failure class for the raw record's context and the host's bounded summary.
    pub fn format_failure(self) -> Option<&'static str> {
        match self {
            Self::NotAnEnvelope => None,
            Self::UnknownVersion => Some("unknown_version"),
            Self::MalformedJson => Some("malformed_json"),
            Self::InvalidPayload => Some("invalid_payload"),
        }
    }
}

/// Parses one logical stderr record as a v1 envelope or classifies why it is not one.
pub fn parse_envelope(record: &str) -> Result<StructuredPayload, EnvelopeRejection> {
    let Some(body) = record.strip_prefix(PLUGIN_LOG_ENVELOPE_V1_PREFIX) else {
        return Err(if record.starts_with(PLUGIN_LOG_ENVELOPE_FAMILY) {
            EnvelopeRejection::UnknownVersion
        } else {
            EnvelopeRejection::NotAnEnvelope
        });
    };
    let Value::Object(mut fields) =
        serde_json::from_str::<Value>(body).map_err(|_| EnvelopeRejection::MalformedJson)?
    else {
        return Err(EnvelopeRejection::InvalidPayload);
    };
    let level = match fields.remove("level") {
        Some(Value::String(level)) => parse_level(&level)?,
        Some(_) | None => return Err(EnvelopeRejection::InvalidPayload),
    };
    let message = match fields.remove("message") {
        Some(Value::String(message)) => message,
        Some(_) | None => return Err(EnvelopeRejection::InvalidPayload),
    };
    let target = optional_identifier(fields.remove("target"))?;
    let method = optional_identifier(fields.remove("method"))?;
    let context = match fields.remove("context") {
        Some(Value::Object(context)) => context,
        None | Some(Value::Null) => Map::new(),
        Some(_) => return Err(EnvelopeRejection::InvalidPayload),
    };
    let error = match fields.remove("error") {
        Some(Value::Object(error)) => Some(error),
        None | Some(Value::Null) => None,
        Some(_) => return Err(EnvelopeRejection::InvalidPayload),
    };
    Ok(StructuredPayload {
        level,
        message,
        target,
        method,
        context,
        error,
    })
}

/// Accepts only the exact uppercase level names the envelope contract defines.
fn parse_level(level: &str) -> Result<LogLevel, EnvelopeRejection> {
    match level {
        "TRACE" => Ok(LogLevel::Trace),
        "DEBUG" => Ok(LogLevel::Debug),
        "INFO" => Ok(LogLevel::Info),
        "WARN" => Ok(LogLevel::Warn),
        "ERROR" => Ok(LogLevel::Error),
        _ => Err(EnvelopeRejection::InvalidPayload),
    }
}

/// Validates an optional identifier field: absent or null is fine, anything else must be a
/// non-empty string within the identifier bound.
fn optional_identifier(value: Option<Value>) -> Result<Option<String>, EnvelopeRejection> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) if !text.is_empty() && text.len() <= MAX_IDENTIFIER_BYTES => {
            Ok(Some(text))
        }
        Some(_) => Err(EnvelopeRejection::InvalidPayload),
    }
}
