//! How one settled history line becomes the Thread event that mirrors it.

use agent_client_protocol_schema::v1::{ContentBlock, SessionUpdate};
use ora_history::{HistoryLine, HistoryRecord};
use ora_node_protocol::{MAX_THREAD_RECORD_BYTES, MAX_VISIBLE_TEXT_BYTES, ThreadEvent, TurnId};
use serde_json::{Map, Value};

/// Fields a truncated record keeps: enough to place it in the conversation and name its kind.
const TRUNCATED_RECORD_FIELDS: [&str; 3] = ["at", "seq", "type"];

/// Mirrors one history line as a Thread event, truncating a record too large for the Thread.
///
/// The event carries the line exactly as the history file holds it, so every Thread record can be
/// found in the file. A record over the protocol limit is replaced by its position and kind alone
/// and marked truncated; the file keeps the original, so nothing is lost, only not relayed.
pub(super) fn thread_event(
    line: &HistoryLine,
    turn_id: Option<TurnId>,
) -> Result<ThreadEvent, serde_json::Error> {
    let record: Map<String, Value> = serde_json::from_value(serde_json::to_value(line)?)?;
    let (visible_text, visible_text_truncated) = visible_assistant_text(line);
    if serde_json::to_vec(&record)?.len() <= MAX_THREAD_RECORD_BYTES {
        return Ok(ThreadEvent {
            turn_id,
            record,
            truncated: false,
            visible_text,
            visible_text_truncated,
            history_sequence: Some(line.seq),
        });
    }
    let record = record
        .into_iter()
        .filter(|(key, _value)| TRUNCATED_RECORD_FIELDS.contains(&key.as_str()))
        .collect();
    Ok(ThreadEvent {
        turn_id,
        record,
        truncated: true,
        visible_text,
        visible_text_truncated,
        history_sequence: Some(line.seq),
    })
}

/// Extracts only assistant prose from the typed settled ACP record, keeping every other update
/// outside the next Agent's context; the original history and Thread record remain untouched.
fn visible_assistant_text(line: &HistoryLine) -> (Option<String>, bool) {
    let HistoryRecord::Update { update, .. } = &line.record else {
        return (None, false);
    };
    let SessionUpdate::AgentMessageChunk(chunk) = update.as_ref() else {
        return (None, false);
    };
    let ContentBlock::Text(text) = &chunk.content else {
        return (None, false);
    };
    if text.text.is_empty() {
        return (None, false);
    }
    let mut end = text.text.len().min(MAX_VISIBLE_TEXT_BYTES);
    while !text.text.is_char_boundary(end) {
        end -= 1;
    }
    (Some(text.text[..end].to_owned()), end < text.text.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol_schema::v1::{
        ContentBlock, ContentChunk, SessionUpdate, TextContent,
    };
    use ora_history::HistoryRecord;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    /// Builds one settled agent message line of the given text.
    fn message(text: &str) -> HistoryLine {
        HistoryLine::new(
            "2026-09-28T10:00:00.000+08:00",
            4,
            HistoryRecord::Update {
                update: Box::new(SessionUpdate::AgentMessageChunk(ContentChunk::new(
                    ContentBlock::Text(TextContent::new(text)),
                ))),
                tool_timing: None,
            },
        )
    }

    /// A record within the limit is relayed exactly as the history file encodes it.
    #[test]
    fn a_record_within_the_limit_is_relayed_as_the_file_holds_it() {
        let line = message("hello");

        let event = thread_event(&line, Some(TurnId::new("turn-1"))).expect("convert line");

        assert_eq!(
            event,
            ThreadEvent {
                turn_id: Some(TurnId::new("turn-1")),
                record: serde_json::from_value(serde_json::to_value(&line).expect("encode"))
                    .expect("object"),
                truncated: false,
                visible_text: Some("hello".to_owned()),
                visible_text_truncated: false,
                history_sequence: Some(4),
            }
        );
    }

    /// An oversized record keeps only its position and kind, and says it was truncated.
    #[test]
    fn an_oversized_record_is_reduced_to_its_position_and_kind() {
        let line = message(&"x".repeat(MAX_THREAD_RECORD_BYTES));

        let event = thread_event(&line, None).expect("convert line");

        assert_eq!(
            event,
            ThreadEvent {
                turn_id: None,
                record: serde_json::from_value(json!({
                    "at": "2026-09-28T10:00:00.000+08:00",
                    "seq": 4,
                    "type": "update",
                }))
                .expect("object"),
                truncated: true,
                visible_text: Some("x".repeat(MAX_VISIBLE_TEXT_BYTES)),
                visible_text_truncated: true,
                history_sequence: Some(4),
            }
        );
    }

    /// A non-assistant text update must remain visible in history but absent from handoff prose.
    #[test]
    fn thought_text_is_not_projected_as_assistant_reply() {
        let line = HistoryLine::new(
            "2026-09-28T10:00:00.000+08:00",
            5,
            HistoryRecord::Update {
                update: Box::new(SessionUpdate::AgentThoughtChunk(ContentChunk::new(
                    ContentBlock::Text(TextContent::new("private intermediate thought")),
                ))),
                tool_timing: None,
            },
        );
        let event = thread_event(&line, Some(TurnId::new("turn-1"))).expect("convert line");
        assert_eq!(event.visible_text, None);
        assert!(!event.visible_text_truncated);
        assert!(event.record.contains_key("update"));
    }

    /// Empty deltas cannot stand in for a reply when building later Agent prompts.
    #[test]
    fn empty_assistant_chunk_does_not_claim_a_visible_reply() {
        let event = thread_event(&message(""), Some(TurnId::new("turn-1"))).expect("convert line");
        assert_eq!(event.visible_text, None);
        assert!(!event.visible_text_truncated);
    }

    /// A multibyte clipping boundary cannot split a Unicode code point or corrupt the next prompt.
    #[test]
    fn visible_projection_stops_at_a_utf8_boundary() {
        let line = message(&"你".repeat(MAX_VISIBLE_TEXT_BYTES / 3 + 1));
        let event = thread_event(&line, None).expect("convert line");
        let visible = event.visible_text.expect("assistant text");
        assert_eq!(visible, "你".repeat(MAX_VISIBLE_TEXT_BYTES / 3));
        assert!(event.visible_text_truncated);
        assert!(!event.truncated);
    }
}
