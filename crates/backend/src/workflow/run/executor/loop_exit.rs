//! Owns cancellation and driver lifetime fencing for a committed Loop break.
use super::WorkflowRunNodeExecutor;
use ora_application::{ExecutionContext, LoopExitCleanup, WorkflowRunEngineRepository};
use ora_contracts::StopSessionRequest;
use ora_db::SqliteWorkflowRunEngineRepository;
use ora_domain::{WorkflowNodeRunId, WorkflowNodeStatus, WorkflowScopeId};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Tracks live drivers, including the window before a new session is attached to its row.
#[derive(Default)]
pub(super) struct LoopExitTasks {
    drivers: Mutex<HashSet<WorkflowNodeRunId>>,
    cleanups: Mutex<HashSet<WorkflowScopeId>>,
}

/// Removes the driver fence on completion, cancellation, or task unwinding.
pub(super) struct DriverGuard {
    tasks: Arc<LoopExitTasks>,
    id: WorkflowNodeRunId,
}
impl Drop for DriverGuard {
    /// A finished driver cannot create or mutate another session after releasing this fence.
    fn drop(&mut self) {
        self.tasks
            .drivers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&self.id);
    }
}
impl LoopExitTasks {
    /// Registers before spawning so cleanup never misses a not-yet-polled driver.
    pub(super) fn register(self: &Arc<Self>, id: WorkflowNodeRunId) -> DriverGuard {
        self.drivers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(id.clone());
        DriverGuard {
            tasks: self.clone(),
            id,
        }
    }
}

/// Starts at most one cleanup per scope; retries after restart use the same persisted fence.
pub(super) fn dispatch(
    executor: &WorkflowRunNodeExecutor,
    context: &ExecutionContext,
    parent_id: &WorkflowNodeRunId,
    scope_id: &WorkflowScopeId,
) -> LoopExitCleanup {
    let tasks = executor.loop_exit_tasks.clone();
    if !tasks
        .cleanups
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(scope_id.clone())
    {
        return LoopExitCleanup::Pending;
    }
    let executor = executor.clone();
    let parent_id = parent_id.clone();
    let scope_id = scope_id.clone();
    let run_id = context.run.id.clone();
    tokio::spawn(async move {
        let result = tokio::time::timeout(Duration::from_secs(30), drain(&executor, &scope_id))
            .await
            .unwrap_or_else(|_| Err("Loop exit timed out while stopping active sessions".into()));
        let callback = executor.callback.clone();
        let callback_scope = scope_id.clone();
        let _ = tokio::task::spawn_blocking(move || {
            callback.finish_loop_exit(&run_id, &parent_id, &callback_scope, result)
        })
        .await;
        tasks
            .cleanups
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&scope_id);
    });
    LoopExitCleanup::Pending
}

/// Rechecks attachment races until all cancelled drivers and their bound sessions are stopped.
async fn drain(
    executor: &WorkflowRunNodeExecutor,
    scope_id: &WorkflowScopeId,
) -> Result<(), String> {
    let repository = SqliteWorkflowRunEngineRepository::new(executor.pool.clone());
    let mut stopped = HashSet::new();
    loop {
        let nodes = repository
            .list_node_runs_in_scope(scope_id)
            .map_err(|error| error.to_string())?;
        let cancelled: Vec<_> = nodes
            .iter()
            .filter(|node| node.status == WorkflowNodeStatus::Cancelled)
            .collect();
        for node in &cancelled {
            if let Some(session_id) = &node.session_id
                && !stopped.contains(session_id)
            {
                executor
                    .agent_runtime
                    .stop_session(StopSessionRequest {
                        session_id: session_id.to_string(),
                    })
                    .await
                    .map_err(|error| error.to_string())?;
                stopped.insert(session_id.clone());
            }
        }
        let active = {
            let drivers = executor
                .loop_exit_tasks
                .drivers
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            cancelled.iter().any(|node| drivers.contains(&node.id))
        };
        if !active {
            return Ok(());
        }
        // Driver callbacks acquire the run gate independently; never wait under that gate.
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}
