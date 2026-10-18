//! Bounded memory-only snapshots shared by session actors and the transport writer.
use ora_history::AssembledRecord;
use ora_node_protocol::{
    CURRENT_PROTOCOL_VERSION, ExecutionId, OperationId, ThreadPreview, ThreadPreviewBlock,
    ThreadPreviewMessage, TurnId,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

#[derive(Clone, Default)]
pub(super) struct PreviewBuffer(Arc<Mutex<HashMap<ExecutionId, ThreadPreviewMessage>>>);

impl PreviewBuffer {
    /// Replaces one execution's pending set. Oversized blocks wait for durable history instead of
    /// creating unbounded allocations or silently pretending a partial snapshot is complete.
    pub(super) fn replace(
        &self,
        operation: &OperationId,
        execution: &ExecutionId,
        turn: Option<TurnId>,
        records: &[AssembledRecord],
    ) {
        let mut snapshots = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if !snapshots.contains_key(execution) && snapshots.len() >= 64 {
            return;
        }
        let version = snapshots
            .get(execution)
            .map_or(1, |prior| prior.payload.version.saturating_add(1));
        let mut blocks = Vec::new();
        let mut bytes = 0;
        let mut incomplete = false;
        let mut encoded = vec![0_u8; 65536];
        for record in records {
            let mut writer = std::io::Cursor::new(encoded.as_mut_slice());
            if serde_json::to_writer(&mut writer, &record.record).is_err() {
                incomplete = true;
                continue;
            }
            let size = writer.position() as usize;
            // Keep room for the envelope and block identities below the 256 KiB wire cap.
            if blocks.len() >= 32 || bytes + size > 240 * 1024 {
                incomplete = true;
                continue;
            }
            let Ok(record_value) = serde_json::from_slice(&encoded[..size]) else {
                incomplete = true;
                continue;
            };
            bytes += size;
            blocks.push(ThreadPreviewBlock {
                history_sequence: record.seq,
                turn_id: turn.clone(),
                record: record_value,
            });
        }
        snapshots.insert(
            execution.clone(),
            ThreadPreviewMessage {
                protocol_version: CURRENT_PROTOCOL_VERSION,
                operation_id: operation.clone(),
                execution_id: execution.clone(),
                payload: ThreadPreview {
                    version,
                    blocks,
                    incomplete,
                },
            },
        );
    }

    /// Returns only the latest replacement for reconnect or a batch flush; no delta queue exists.
    pub(super) fn snapshots(&self) -> Vec<ThreadPreviewMessage> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .cloned()
            .collect()
    }

    /// Ended executions have durable terminal evidence and no longer own preview memory.
    pub(super) fn remove(&self, execution: &ExecutionId) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(execution);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol_schema::v1::{
        ContentBlock, ContentChunk, SessionUpdate, TextContent,
    };
    use ora_history::HistoryRecord;
    use pretty_assertions::assert_eq;

    /// Replacements keep one stable history position and one bounded cache entry per execution.
    #[test]
    fn snapshots_replace_and_report_oversized_content_without_queueing_deltas() {
        let buffer = PreviewBuffer::default();
        let operation = OperationId::new("run");
        let execution = ExecutionId::new("execution");
        let record = |text: &str| AssembledRecord {
            seq: 9,
            record: HistoryRecord::Update {
                update: Box::new(SessionUpdate::AgentMessageChunk(ContentChunk::new(
                    ContentBlock::Text(TextContent::new(text)),
                ))),
                tool_timing: None,
            },
        };
        buffer.replace(
            &operation,
            &execution,
            Some(TurnId::new("turn")),
            &[record("first")],
        );
        buffer.replace(
            &operation,
            &execution,
            Some(TurnId::new("turn")),
            &[record("replacement")],
        );
        let snapshots = buffer.snapshots();
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].payload.version, 2);
        assert_eq!(snapshots[0].payload.blocks[0].history_sequence, 9);
        buffer.replace(
            &operation,
            &execution,
            /*turn*/ None,
            &[record(&"x".repeat(65537))],
        );
        assert_eq!(
            buffer.snapshots()[0].payload,
            ThreadPreview {
                version: 3,
                blocks: Vec::new(),
                incomplete: true
            }
        );
        buffer.remove(&execution);
        assert!(buffer.snapshots().is_empty());
    }
}
