//! Ephemeral snapshots use the registered session identity and current lease, never receipts.
use super::*;

impl CloudStore {
    /// Publishes once: the connection-owned relay coalesces newer snapshots while this awaits.
    pub(in crate::cloud) async fn thread_preview(
        &self,
        session: &NodeRuntimeIdentity,
        preview: &ThreadPreviewMessage,
    ) -> Result<(), Error> {
        preview.validate()?;
        let record = self
            .agent_record(session, &preview.operation_id, &preview.execution_id)
            .await?;
        if record.result.is_some() {
            return Err(Error::Conflict);
        }
        let blocks = preview
            .payload
            .blocks
            .iter()
            .map(|block| {
                Ok(proto::ThreadPreviewBlock {
                    history_sequence: block.history_sequence,
                    turn_id: block
                        .turn_id
                        .as_ref()
                        .map_or_else(String::new, |id| id.as_str().into()),
                    record: serde_json::to_string(&block.record)?,
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        let request = proto::PublishThreadPreviewRequest {
            epoch: self.epoch()?,
            operation_id: record.operation_id,
            execution_id: record.execution_id,
            version: i64::try_from(preview.payload.version).map_err(|_| Error::Conflict)?,
            blocks,
            incomplete: preview.payload.incomplete,
        };
        // A preview is replaceable and has no submission identity. Do not run the durable-write
        // retry loop: replaying an obsolete snapshot delays the latest one and creates no Ack.
        fault::read(self.agents().publish_thread_preview(self.request(request)))
            .await
            .map(drop)
            .map_err(|verdict| self.settle(verdict))
    }
}

#[cfg(test)]
mod tests;
