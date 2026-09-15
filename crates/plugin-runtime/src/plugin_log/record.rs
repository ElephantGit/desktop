//! Turns one logical stderr record into the JSONL line the host persists.
//!
//! The persisted shape reuses the Ora runtime log envelope (`timestamp`, `level`, `target`,
//! `message`, optional `method`/`context`/`error`) so the same tooling can read both, but the
//! trusted fields are always written here by the host: the plugin may describe itself in
//! `context`, and it may even name `plugin_id`, yet what lands on disk is the identity the
//! process was launched under.

use ora_logging::LogLevel;
use ora_utils::text::{ByteRendering, LineFrame, render_bytes_lossless};
use serde_json::{Map, Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::plugin_log::envelope::{EnvelopeRejection, parse_envelope};

/// Target of every structured record whose payload named none.
pub const DEFAULT_PLUGIN_TARGET: &str = "plugin";

/// Target of every raw record: unstructured bytes the plugin or its dependencies wrote.
pub const RAW_STDERR_TARGET: &str = "plugin.stderr";

/// Identity the host binds to every record of one process generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordOrigin {
    pub plugin_id: String,
    pub generation: u64,
}

/// One record ready to be filtered and persisted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginLogRecord {
    pub timestamp: OffsetDateTime,
    pub level: LogLevel,
    pub target: String,
    pub message: String,
    pub method: Option<String>,
    pub context: Map<String, Value>,
    pub error: Option<Map<String, Value>>,
}

/// The decoded record plus the format-failure class the host may count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedRecord {
    pub record: PluginLogRecord,
    pub format_failure: Option<&'static str>,
}

impl PluginLogRecord {
    /// Renders the record as one JSON line, newline included.
    pub fn to_json_line(&self) -> String {
        let mut payload = Map::new();
        // RFC 3339 formatting of an offset datetime cannot fail; an empty string would only
        // ever appear if `time` changed that contract, and it still keeps the line well-formed.
        let timestamp = self.timestamp.format(&Rfc3339).unwrap_or_default();
        payload.insert("timestamp".to_string(), Value::String(timestamp));
        payload.insert(
            "level".to_string(),
            Value::String(self.level.as_upper_str().to_string()),
        );
        payload.insert("target".to_string(), Value::String(self.target.clone()));
        payload.insert("message".to_string(), Value::String(self.message.clone()));
        if let Some(method) = &self.method {
            payload.insert("method".to_string(), Value::String(method.clone()));
        }
        payload.insert("context".to_string(), Value::Object(self.context.clone()));
        if let Some(error) = &self.error {
            payload.insert("error".to_string(), Value::Object(error.clone()));
        }
        let mut line = Value::Object(payload).to_string();
        line.push('\n');
        line
    }
}

/// Decodes one frame into a record, taking the structured path when the frame is a valid v1
/// envelope and the raw path otherwise.
///
/// `now` is injected so the pipeline stamps records at receipt with the host clock while unit
/// tests stay deterministic.
pub fn decode_frame(frame: LineFrame, origin: &RecordOrigin, now: OffsetDateTime) -> DecodedRecord {
    match frame {
        LineFrame::Line(bytes) => decode_line(&strip_carriage_return(bytes), origin, now),
        LineFrame::Fragment {
            sequence,
            index,
            last,
            bytes,
        } => {
            let bytes = if last {
                strip_carriage_return(bytes)
            } else {
                bytes
            };
            let mut record = raw_record(&bytes, origin, now);
            record.context.insert(
                "fragment".to_string(),
                json!({ "sequence": sequence, "index": index, "last": last }),
            );
            DecodedRecord {
                record,
                format_failure: None,
            }
        }
    }
}

/// Decodes one complete logical record.
fn decode_line(bytes: &[u8], origin: &RecordOrigin, now: OffsetDateTime) -> DecodedRecord {
    // An envelope is UTF-8 by contract, so bytes that are not valid UTF-8 can only be raw.
    let rejection = match std::str::from_utf8(bytes) {
        Ok(text) => match parse_envelope(text) {
            Ok(payload) => {
                let mut context = payload.context;
                stamp_origin(&mut context, origin);
                return DecodedRecord {
                    record: PluginLogRecord {
                        timestamp: now,
                        level: payload.level,
                        target: payload
                            .target
                            .unwrap_or_else(|| DEFAULT_PLUGIN_TARGET.to_string()),
                        message: payload.message,
                        method: payload.method,
                        context,
                        error: payload.error,
                    },
                    format_failure: None,
                };
            }
            Err(rejection) => rejection,
        },
        Err(_) => EnvelopeRejection::NotAnEnvelope,
    };
    let mut record = raw_record(bytes, origin, now);
    let format_failure = rejection.format_failure();
    if let Some(class) = format_failure {
        record.context.insert(
            "format_failure".to_string(),
            Value::String(class.to_string()),
        );
    }
    DecodedRecord {
        record,
        format_failure,
    }
}

/// Builds the raw fallback record, preserving invalid bytes reversibly and saying so.
fn raw_record(bytes: &[u8], origin: &RecordOrigin, now: OffsetDateTime) -> PluginLogRecord {
    let (message, rendering) = render_bytes_lossless(bytes);
    let mut context = Map::new();
    if rendering == ByteRendering::Escaped {
        context.insert(
            "encoding".to_string(),
            Value::String("escaped-bytes".to_string()),
        );
    }
    stamp_origin(&mut context, origin);
    PluginLogRecord {
        timestamp: now,
        level: LogLevel::Info,
        target: RAW_STDERR_TARGET.to_string(),
        message: message.into_owned(),
        method: None,
        context,
        error: None,
    }
}

/// Writes the host-known identity last so nothing the plugin supplied can survive under the
/// trusted keys.
fn stamp_origin(context: &mut Map<String, Value>, origin: &RecordOrigin) {
    context.insert(
        "plugin_id".to_string(),
        Value::String(origin.plugin_id.clone()),
    );
    context.insert("generation".to_string(), json!(origin.generation));
}

/// Drops one trailing `\r` so CRLF producers do not leave a stray control character behind.
fn strip_carriage_return(mut bytes: Vec<u8>) -> Vec<u8> {
    if bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
    bytes
}
