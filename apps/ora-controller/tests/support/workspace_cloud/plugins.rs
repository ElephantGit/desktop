//! Plugin evidence rules used by the Workspace Controller scenarios.
use super::*;

impl WorkspaceCloud {
    /// Configures the exact snapshot input before accepting a Workspace operation.
    pub fn plan_plugins(&self, input: proto::ExecutionInput) {
        self.lock().plugin_input = Some(input);
    }

    /// Returns durable plugin dispatches, including completed evidence.
    pub fn plugin_records(&self) -> Vec<proto::ExecutionRecord> {
        self.lock()
            .clones
            .iter()
            .filter(|r| is_plugin(r))
            .cloned()
            .collect()
    }
}

/// Separates execution families in the operation snapshot.
pub(super) fn is_plugin(record: &proto::ExecutionRecord) -> bool {
    matches!(
        record.input.as_ref().and_then(|i| i.spec.as_ref()),
        Some(
            proto::execution_input::Spec::InstallPlugins(_)
                | proto::execution_input::Spec::RemovePlugins(_)
        )
    )
}

/// Admission depends on durable item evidence, never on dispatch or a whole-execution failure.
pub(super) fn advance(state: &mut State, operation: &str) -> Result<(), Status> {
    if !state
        .clones
        .iter()
        .rev()
        .find(|r| r.operation_id == operation && is_plugin(r))
        .and_then(|r| r.result.as_ref())
        .is_some_and(|r| {
            matches!(
                r.outcome,
                Some(proto::execution_result::Outcome::PluginsResult(_))
            )
        })
    {
        return Err(conflict("plugins_incomplete"));
    }
    state.workspace.observed_state = "ready".into();
    state.workspace.admission_open = true;
    Ok(())
}
