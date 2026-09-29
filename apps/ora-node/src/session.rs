//! Agent session executions: one agent plugin running in one checkout, its settled records
//! relayed as Thread events, and the session commands the ledger queued run in order.
//!
//! Each execution composes its own plugin lifecycle and agent runtime from `ora-agent-runtime`,
//! so it starts only the plugin it names and exports only its own Git identity. A session never
//! outlives the Node process: a restarted Node ends every session it finds unfinished as
//! interrupted instead of resuming it.

mod driver;
mod host;
mod ports;
mod queue;
mod thread;

pub use ports::{
    CheckoutResolver, CommandSettlement, HistoryUnavailable, PluginCatalog, QueuedCommand,
    SessionCommand, SessionHost, SessionLedger,
};

use chrono_tz::Tz;
use ora_node_protocol::{
    AgentSessionEndReason, AgentSessionEnded, AgentSessionSpec, ExecutionId, NodeRuntimeIdentity,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;
use tokio::sync::Notify;

/// Where and with what a Node runs its session executions.
#[derive(Clone, Debug)]
pub struct SessionConfig {
    /// The Node data directory: agent plugins are installed under its `plugins/` root and session
    /// histories are kept under `sessions/`.
    pub home_directory: PathBuf,
    /// The Deno executable agent plugins run on.
    pub deno_path: PathBuf,
    /// The timezone scheduled runtime work is evaluated in.
    pub timezone: Tz,
    /// How long a session waits for its agent to become ready before it ends as failed.
    pub agent_ready_timeout: Duration,
}

/// State shared by every session execution of one Node.
pub(crate) struct Shared<L, C, P> {
    config: SessionConfig,
    node: NodeRuntimeIdentity,
    ledger: L,
    checkouts: C,
    catalog: P,
    /// Wake handles of the sessions running in this process, by execution.
    live: Mutex<HashMap<ExecutionId, Arc<Notify>>>,
}

impl<L, C, P> Shared<L, C, P> {
    /// Session histories live beside the plugin root in the Node data directory.
    fn sessions_root(&self) -> PathBuf {
        self.config.home_directory.join("sessions")
    }

    /// Forgets a session whose history nothing writes to anymore.
    fn release(&self, execution: &ExecutionId) {
        self.live
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(execution);
    }

    /// Reports whether a session of this execution is running in this process.
    fn is_live(&self, execution: &ExecutionId) -> bool {
        self.live
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains_key(execution)
    }
}

/// Runs the Node's Agent session executions.
pub struct AgentSessions<L, C, P> {
    shared: Arc<Shared<L, C, P>>,
}

impl<L, C, P> Clone for AgentSessions<L, C, P> {
    fn clone(&self) -> Self {
        Self {
            shared: Arc::clone(&self.shared),
        }
    }
}

impl<L, C, P> AgentSessions<L, C, P>
where
    L: SessionLedger,
    C: CheckoutResolver,
    P: PluginCatalog,
{
    /// Composes session execution over the ledger, clone bookkeeping and plugin catalog of the
    /// Node incarnation `node`, which every terminal result reports.
    pub fn new(
        config: SessionConfig,
        node: NodeRuntimeIdentity,
        ledger: L,
        checkouts: C,
        catalog: P,
    ) -> Self {
        Self {
            shared: Arc::new(Shared {
                config,
                node,
                ledger,
                checkouts,
                catalog,
                live: Mutex::new(HashMap::new()),
            }),
        }
    }
}

impl<L, C, P> SessionHost for AgentSessions<L, C, P>
where
    L: SessionLedger,
    C: CheckoutResolver,
    P: PluginCatalog,
{
    /// Spawns the session on the current Tokio runtime; a repeated start of a live execution is
    /// ignored, because its input is persisted once and the running session already owns it.
    fn start(&self, execution: ExecutionId, spec: AgentSessionSpec) {
        let wake = Arc::new(Notify::new());
        {
            let mut live = self
                .shared
                .live
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if live.contains_key(&execution) {
                return;
            }
            live.insert(execution.clone(), Arc::clone(&wake));
        }
        tokio::spawn(driver::run(Arc::clone(&self.shared), execution, spec, wake));
    }

    /// A wake that finds no live session is dropped: the session already ended, and the ledger
    /// settles whatever it left queued with the terminal result.
    fn command_arrived(&self, execution: &ExecutionId) {
        if let Some(wake) = self
            .shared
            .live
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(execution)
        {
            wake.notify_one();
        }
    }

    /// No agent runtime survives a Node restart: the history's only writer ended with the old
    /// process, so what the file holds is final and there is nothing to resume or seal further.
    /// The plugin's stdio ended with the old process too, which ends a well-behaved plugin; a
    /// descendant that ignores that is not reclaimed until process containment covers plugins.
    fn recover_interrupted(&self, _execution: &ExecutionId) -> AgentSessionEnded {
        AgentSessionEnded {
            node: self.shared.node.clone(),
            reason: AgentSessionEndReason::Interrupted,
            detail: None,
        }
    }

    fn sealed_history(&self, execution: &ExecutionId) -> Result<PathBuf, HistoryUnavailable> {
        if self.shared.is_live(execution) {
            return Err(HistoryUnavailable);
        }
        let path = ora_history::history_path(&self.shared.sessions_root(), execution.as_str())
            .map_err(|_invalid| HistoryUnavailable)?;
        if path.is_file() {
            Ok(path)
        } else {
            Err(HistoryUnavailable)
        }
    }
}
