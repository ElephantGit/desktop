//! Plugin execution input, terminal evidence and delivery are committed before external use.
use crate::*;
use ora_node_protocol::*;
use rusqlite::{OptionalExtension, params};

/// Input and state of one accepted plugin execution; its terminal result survives acknowledgement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginExecution {
    pub command: PluginCommand,
    pub state: ExecutionState,
}

impl<G: WriteGuard> NodeDatabase<G> {
    /// Reads by both identity keys so another capability cannot adopt the same execution.
    pub fn find_plugins(
        &self,
        operation: &OperationId,
        execution: &ExecutionId,
    ) -> Result<Option<PluginExecution>, Error> {
        if self
            .identity_kind(operation, execution)?
            .is_some_and(|kind| kind != "plugin")
        {
            return Err(Error::IdentityConflict);
        }
        let row: Option<(String, String, Option<String>)> = self
            .connection
            .query_row(
                "SELECT input,state,result FROM plugin_executions WHERE execution=?1",
                [execution.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        row.map(decode).transpose()
    }

    /// Accepts a local execution only when this home has never required runtime control.
    pub fn accept_plugins(&mut self, command: &PluginCommand) -> Result<PluginExecution, Error> {
        let fenced: bool = self.connection.query_row("SELECT EXISTS(SELECT 1 FROM runtime_enforcement) OR EXISTS(SELECT 1 FROM runtime_binding)", [], |r| r.get(0))?;
        if fenced {
            return Err(Error::InvalidTransition);
        }
        self.accept_plugin_input(command, /*permit*/ None)
    }

    /// Persists the exact control responsibility together with the input before any download.
    pub fn accept_controlled_plugins(
        &mut self,
        envelope: &ControlledPlugins,
    ) -> Result<PluginExecution, Error> {
        envelope.validate()?;
        self.validate_runtime_permit(&envelope.binding)?;
        self.accept_plugin_input(&envelope.command, Some(&envelope.binding))
    }

    /// Deduplicates the full immutable input, then records admission and ownership atomically.
    fn accept_plugin_input(
        &mut self,
        command: &PluginCommand,
        permit: Option<&RuntimeBinding>,
    ) -> Result<PluginExecution, Error> {
        command.validate()?;
        if command.node_id() != &self.node_id {
            return Err(Error::NodeMismatch);
        }
        if let Some(record) = self.find_plugins(command.operation_id(), command.execution_id())? {
            return if record.command == *command {
                Ok(record)
            } else {
                Err(Error::IdentityConflict)
            };
        }
        if permit.is_some() && !self.unfinished_runtime_executions()?.is_empty() {
            return Err(Error::ResourceConflict);
        }
        self.guard.before_write(WritePoint::Accept)?;
        let tx = self.connection.transaction()?;
        tx.execute(
            "INSERT INTO execution_identities VALUES(?1,?2,'plugin')",
            params![
                command.operation_id().as_str(),
                command.execution_id().as_str()
            ],
        )?;
        tx.execute(
            "INSERT INTO plugin_executions VALUES(?1,?2,'accepted',NULL)",
            params![
                command.execution_id().as_str(),
                serde_json::to_string(command)?
            ],
        )?;
        if let Some(permit) = permit {
            tx.execute(
                "INSERT INTO execution_control(execution,permit) VALUES(?1,?2)",
                params![
                    command.execution_id().as_str(),
                    serde_json::to_string(permit)?
                ],
            )?;
        }
        tx.commit()?;
        Ok(PluginExecution {
            command: command.clone(),
            state: ExecutionState::Accepted,
        })
    }

    /// Rechecks authority at the execution entrance. Old incarnations cannot resume mutations.
    pub fn start_plugins(
        &mut self,
        command: &PluginCommand,
        incarnation: &NodeIncarnationId,
    ) -> Result<bool, Error> {
        let record = self
            .find_plugins(command.operation_id(), command.execution_id())?
            .ok_or(Error::IdentityConflict)?;
        if record.command != *command || matches!(record.state, ExecutionState::Completed(_)) {
            return Err(Error::InvalidTransition);
        }
        if !self
            .start_controlled_execution_for(command.execution_id(), Some(incarnation.as_str()))?
        {
            return Ok(false);
        }
        self.guard.before_write(WritePoint::Progress)?;
        self.connection.execute(
            "UPDATE plugin_executions SET state='running' WHERE execution=?1",
            [command.execution_id().as_str()],
        )?;
        Ok(true)
    }

    /// Commits one immutable terminal result and its single replay envelope in one transaction.
    pub fn complete_plugins(
        &mut self,
        command: &PluginCommand,
        result: PluginExecutionResult,
    ) -> Result<(), Error> {
        let record = self
            .find_plugins(command.operation_id(), command.execution_id())?
            .ok_or(Error::IdentityConflict)?;
        if record.command != *command || !command.accepts_result(&result) {
            return Err(Error::IdentityConflict);
        }
        if matches!(record.state, ExecutionState::Completed(_)) {
            return Err(Error::InvalidTransition);
        }
        let event = PluginsResultMessage {
            protocol_version: CURRENT_PROTOCOL_VERSION,
            operation_id: command.operation_id().clone(),
            execution_id: command.execution_id().clone(),
            sequence: Sequence::new(/*value*/ 1),
            payload: result.clone(),
        };
        event.validate()?;
        self.guard.before_write(WritePoint::Complete)?;
        let tx = self.connection.transaction()?;
        tx.execute(
            "UPDATE plugin_executions SET state='completed',result=?1 WHERE execution=?2",
            params![
                serde_json::to_string(&result)?,
                command.execution_id().as_str()
            ],
        )?;
        self.guard.before_write(WritePoint::Outbox)?;
        tx.execute(
            "INSERT INTO plugin_outbox VALUES(?1,?2)",
            params![
                command.execution_id().as_str(),
                serde_json::to_string(&NodeToControllerMessage::PluginsResult(event))?
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Lists only unfinished executions, in original admission order, for deterministic replay.
    pub fn recoverable_plugins(&self) -> Result<Vec<PluginExecution>, Error> {
        self.connection.prepare("SELECT input,state,result FROM plugin_executions WHERE state<>'completed' ORDER BY rowid")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .map(|row| decode(row?)).collect()
    }

    /// A query cannot release the event: only the exact terminal sequence may do so.
    pub(crate) fn acknowledge_plugins(&mut self, ack: &EventAckMessage) -> Result<(), Error> {
        let record = self
            .find_plugins(&ack.operation_id, &ack.execution_id)?
            .ok_or(Error::InvalidAck)?;
        if ack.sequence != Sequence::new(/*value*/ 1)
            || !matches!(record.state, ExecutionState::Completed(_))
        {
            return Err(Error::InvalidAck);
        }
        self.guard.before_write(WritePoint::Acknowledge)?;
        self.connection.execute(
            "DELETE FROM plugin_outbox WHERE execution=?1",
            [ack.execution_id.as_str()],
        )?;
        Ok(())
    }
}

/// Rejects corrupt rows rather than inferring a terminal outcome from delivery state.
fn decode(
    (input, state, result): (String, String, Option<String>),
) -> Result<PluginExecution, Error> {
    let state = match (state.as_str(), result) {
        ("accepted", None) => ExecutionState::Accepted,
        ("running", None) => ExecutionState::Running,
        ("completed", Some(result)) => {
            ExecutionState::Completed(ExecutionResult::Plugin(serde_json::from_str(&result)?))
        }
        _ => return Err(Error::InvalidSchema),
    };
    Ok(PluginExecution {
        command: serde_json::from_str(&input)?,
        state,
    })
}
