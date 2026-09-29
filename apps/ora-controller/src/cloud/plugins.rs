//! Plugin execution ownership remains in Cloud; the adapter never keeps a second durable ledger.
pub(super) mod mapping;
use super::{CloudStore, fault};
use crate::*;
use ora_controller_proto::v1 as proto;

impl CloudStore {
    /// Lists unfinished plugin commands alongside, but independently of, clone intake.
    pub(super) async fn pending_plugin_commands(
        &self,
        node: &NodeId,
    ) -> Result<Vec<PluginCommand>, Error> {
        let response = fault::read(async {
            self.executions()
                .list_pending_dispatches(self.request(proto::ListPendingDispatchesRequest {
                    node_id: node.as_str().into(),
                }))
                .await
        })
        .await
        .map_err(|v| self.settle(v))?;
        response
            .records
            .iter()
            .filter(|r| mapping::is_plugin(r))
            .map(|r| mapping::command(r, node))
            .collect()
    }

    /// Proves operation, execution, target and family before trusting a Node reply.
    pub(super) async fn plugin_command(
        &self,
        session: &NodeRuntimeIdentity,
        operation: &OperationId,
        execution: &ExecutionId,
    ) -> Result<Option<PluginCommand>, Error> {
        let record = self.record(execution).await?.ok_or(Error::Conflict)?;
        if record.node_operation_id != operation.as_str()
            || record.node_id != session.node_id.as_str()
        {
            return Err(Error::Conflict);
        }
        if !mapping::is_plugin(&record) {
            return Ok(None);
        }
        Ok(Some(mapping::command(&record, &session.node_id)?))
    }

    /// Writes a fact learned by query without an event acknowledgement.
    pub(super) async fn plugin_query(
        &self,
        session: &NodeRuntimeIdentity,
        operation: &OperationId,
        execution: &ExecutionId,
        result: &PluginExecutionResult,
    ) -> Result<(), Error> {
        self.check_plugin_result(session, operation, execution, result)
            .await?;
        let record = self.record(execution).await?.ok_or(Error::Conflict)?;
        let epoch = self.epoch()?;
        fault::write(|submission_id| {
            let request = proto::RecordQueriedResultRequest {
                submission_id,
                epoch,
                operation_id: record.operation_id.clone(),
                execution_id: execution.as_str().into(),
                result: Some(mapping::result(result)),
            };
            async move {
                self.executions()
                    .record_queried_result(self.request(request))
                    .await
            }
        })
        .await
        .map(drop)
        .map_err(|v| self.settle(v))
    }

    /// Commits the exact event envelope and its receipt in Cloud before the session may ACK it.
    pub(super) async fn plugin_event(
        &self,
        session: &NodeRuntimeIdentity,
        event: &PluginsResultMessage,
    ) -> Result<(), Error> {
        event.validate()?;
        if event.sequence != Sequence::new(/*value*/ 1) {
            return Err(Error::Conflict);
        }
        self.check_plugin_result(
            session,
            &event.operation_id,
            &event.execution_id,
            &event.payload,
        )
        .await?;
        let record = self
            .record(&event.execution_id)
            .await?
            .ok_or(Error::Conflict)?;
        let encoded = serde_json::to_vec(event)?;
        let epoch = self.epoch()?;
        fault::write(|submission_id| {
            let request = proto::TakeOverNodeEventRequest {
                submission_id,
                epoch,
                operation_id: record.operation_id.clone(),
                execution_id: event.execution_id.as_str().into(),
                sequence: event.sequence.value(),
                result: Some(mapping::result(&event.payload)),
                event: encoded.clone(),
            };
            async move {
                self.executions()
                    .take_over_node_event(self.request(request))
                    .await
            }
        })
        .await
        .map(drop)
        .map_err(|v| self.settle(v))
    }

    /// Rejects missing, extra or mismatched item evidence as a whole, before writing anything.
    async fn check_plugin_result(
        &self,
        session: &NodeRuntimeIdentity,
        operation: &OperationId,
        execution: &ExecutionId,
        result: &PluginExecutionResult,
    ) -> Result<(), Error> {
        let command = self
            .plugin_command(session, operation, execution)
            .await?
            .ok_or(Error::Conflict)?;
        if !command.accepts_result(result) {
            return Err(Error::Conflict);
        }
        Ok(())
    }

    /// Records the snapshot input verbatim. Cloud assigns the stable Node-local operation ID.
    pub(super) async fn record_plugins(
        &self,
        epoch: i64,
        operation: &str,
        node: &NodeId,
        input: &proto::ExecutionInput,
    ) -> Result<PluginCommand, Error> {
        let execution = ExecutionId::new(uuid::Uuid::new_v4().to_string());
        mapping::from_input(input, node, OperationId::new(operation), execution.clone())?;
        let response = fault::write(|submission_id| {
            let request = proto::RecordDispatchRequest {
                submission_id,
                epoch,
                operation_id: operation.into(),
                execution_id: execution.as_str().into(),
                node_id: node.as_str().into(),
                input: Some(input.clone()),
            };
            async move {
                self.executions()
                    .record_dispatch(self.request(request))
                    .await
            }
        })
        .await
        .map_err(|v| self.settle(v))?;
        let record = response.record.ok_or(Error::Conflict)?;
        if record.input.as_ref() != Some(input)
            || record.operation_id != operation
            || record.execution_id != execution.as_str()
        {
            return Err(Error::Conflict);
        }
        mapping::command(&record, node)
    }
}
