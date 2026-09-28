//! In-memory host for runtime tests.
//!
//! Each fake keeps the observable contract of the interface it stands in for — a store that
//! hides soft-deleted rows, an attach that reports only what is installed, a setup that resolves
//! an empty MCP set — so a test exercising the runtime through it fails for the runtime's reasons
//! rather than the fake's.

use crate::host::{
    AgentAttach, AgentPluginAttachment, AgentRuntimeHost, RuntimeEvents, SessionSetup,
    SessionStore, WorkspaceDirectory,
};
use crate::session_setup::{AgentSessionMcpCapabilities, SessionMcpRevision, SessionMcpSnapshot};
use crate::{AgentRuntimeManager, AgentRuntimeSetup, RuntimeError};
use ora_domain::{
    AgentRef, HistoryState, PluginId, Session, SessionId, SessionMcpSelection, SessionStatus,
    SessionTitle, WorkspaceId,
};
use ora_effect::ConsumerDeclaration;
use ora_history::HistoryLine;
use ora_plugin_lifecycle::ConnectionError;
use ora_scheduler::Scheduler;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use thiserror::Error;

/// Composes the in-memory fakes into one runtime host.
pub(crate) struct TestHost;

impl AgentRuntimeHost for TestHost {
    type Store = MemorySessionStore;
    type Attach = InstalledAgents;
    type Setup = EmptySessionSetup;
    type Events = RecordedEvents;
    type Directory = FixedDirectory;
}

/// The only failure the in-memory store reports: the addressed row is not visible.
#[derive(Debug, Error)]
#[error("session {0} is not stored")]
pub(crate) struct MissingSession(String);

/// Session rows in insertion order, with soft deletion kept as a flag like the durable store.
#[derive(Clone, Default)]
pub(crate) struct MemorySessionStore {
    rows: Arc<Mutex<Vec<Session>>>,
}

impl MemorySessionStore {
    /// Replaces one visible row through `update`, returning the stored result.
    fn update(
        &self,
        session_id: &SessionId,
        update: impl FnOnce(Session) -> Session,
    ) -> Result<Session, MissingSession> {
        let mut rows = self.rows.lock().unwrap_or_else(PoisonError::into_inner);
        let row = rows
            .iter_mut()
            .find(|row| row.id == *session_id && !row.audit_fields.is_deleted)
            .ok_or_else(|| MissingSession(session_id.to_string()))?;
        *row = update(row.clone());
        Ok(row.clone())
    }
}

impl SessionStore for MemorySessionStore {
    type Error = MissingSession;

    fn create_session(&self, session: Session) -> Result<Session, MissingSession> {
        self.rows
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(session.clone());
        Ok(session)
    }

    fn find_session(&self, session_id: &SessionId) -> Result<Option<Session>, MissingSession> {
        Ok(self
            .rows
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .find(|row| row.id == *session_id && !row.audit_fields.is_deleted)
            .cloned())
    }

    fn list_sessions(&self) -> Result<Vec<Session>, MissingSession> {
        Ok(self
            .rows
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|row| !row.audit_fields.is_deleted)
            .cloned()
            .collect())
    }

    fn update_session_title(
        &self,
        session_id: &SessionId,
        title: &SessionTitle,
        now: i64,
    ) -> Result<Session, MissingSession> {
        self.update(session_id, |row| {
            let mut row = row.with_title(Some(title.clone()));
            row.audit_fields.updated_at = now;
            row
        })
    }

    fn update_session_status(
        &self,
        session_id: &SessionId,
        status: SessionStatus,
        now: i64,
    ) -> Result<Session, MissingSession> {
        self.update(session_id, |row| row.with_status(status, now))
    }

    fn update_session_binding(
        &self,
        session_id: &SessionId,
        agent_ref: AgentRef,
        agent_session_id: &str,
        now: i64,
    ) -> Result<Session, MissingSession> {
        self.update(session_id, |row| {
            row.with_binding(agent_ref, agent_session_id, now)
        })
    }

    fn update_session_history_state(
        &self,
        session_id: &SessionId,
        history_state: &HistoryState,
        now: i64,
    ) -> Result<Session, MissingSession> {
        self.update(session_id, |row| {
            row.with_history_state(history_state.clone(), now)
        })
    }

    fn soft_delete_session(
        &self,
        session_id: &SessionId,
        deleted_at: i64,
    ) -> Result<bool, MissingSession> {
        Ok(self
            .update(session_id, |mut row| {
                row.audit_fields.is_deleted = true;
                row.audit_fields.updated_at = deleted_at;
                row
            })
            .is_ok())
    }
}

/// Reports a fixed set of installed agent plugins, none of which ever starts.
///
/// Attaching always finds no process, which the supervisor treats as an agent that is not
/// installed yet and keeps retrying quietly — the same state an installed but disabled package
/// leaves Desktop in.
#[derive(Default)]
pub(crate) struct InstalledAgents {
    plugins: Mutex<Vec<PluginId>>,
}

impl InstalledAgents {
    /// Reports exactly `plugins` as installed agent packages.
    pub(crate) fn new(plugins: Vec<PluginId>) -> Self {
        Self {
            plugins: Mutex::new(plugins),
        }
    }

    /// Adds one agent package, as an install while the host is running would.
    pub(crate) fn install(&self, plugin_id: PluginId) {
        self.plugins
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(plugin_id);
    }
}

/// Stopping a plugin that never started cannot fail.
#[derive(Debug, Error)]
#[error("unreachable")]
pub(crate) enum NeverFails {}

impl AgentAttach for InstalledAgents {
    type StopError = NeverFails;

    fn installed_agent_plugins(&self) -> Vec<PluginId> {
        self.plugins
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn is_installed(&self, plugin_id: &PluginId) -> bool {
        self.plugins
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(plugin_id)
    }

    async fn attach_agent(
        &self,
        _plugin_id: &PluginId,
    ) -> Result<AgentPluginAttachment, ConnectionError> {
        Err(ConnectionError::NoProcess)
    }

    async fn stop_plugin(&self, _plugin_id: &PluginId) -> Result<(), NeverFails> {
        Ok(())
    }

    fn replace_agent_effect_declaration(
        &self,
        _plugin_id: PluginId,
        _declaration: Option<ConsumerDeclaration>,
    ) -> Result<(), RuntimeError> {
        Ok(())
    }
}

/// Resolves every session to an empty MCP set.
#[derive(Clone)]
pub(crate) struct EmptySessionSetup {
    selection: SessionMcpSelection,
}

impl Default for EmptySessionSetup {
    fn default() -> Self {
        Self {
            selection: SessionMcpSelection::Automatic,
        }
    }
}

impl SessionSetup for EmptySessionSetup {
    fn with_selection(&self, selection: SessionMcpSelection) -> Self {
        Self { selection }
    }

    fn selection(&self) -> &SessionMcpSelection {
        &self.selection
    }

    fn desired_mcp_revision(&self) -> Result<SessionMcpRevision, RuntimeError> {
        Ok(SessionMcpRevision::default())
    }

    fn resolve_mcp(
        &self,
        _cwd: &Path,
        _capabilities: AgentSessionMcpCapabilities,
    ) -> Result<SessionMcpSnapshot, RuntimeError> {
        Ok(SessionMcpSnapshot::new(
            Vec::new(),
            SessionMcpRevision::default(),
        ))
    }

    fn observe_mcp_health(&self, _session_id: &SessionId, _cwd: &Path) {}

    fn plugin_id_for_agent(&self, agent_ref: &AgentRef) -> Option<PluginId> {
        PluginId::parse(agent_ref.as_str()).ok()
    }
}

/// One lifecycle notification the runtime published.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RecordedEvent {
    SessionTitleUpdated(SessionId),
    AgentModelsInvalidated(AgentRef),
}

/// Keeps every published notification in order for assertions.
///
/// Settled history lines are kept apart from lifecycle notifications, so a test about one never
/// has to account for the other.
#[derive(Clone, Default)]
pub(crate) struct RecordedEvents {
    events: Arc<Mutex<Vec<RecordedEvent>>>,
    settled: Arc<Mutex<Vec<(SessionId, HistoryLine)>>>,
}

impl RecordedEvents {
    /// Returns everything published so far.
    pub(crate) fn published(&self) -> Vec<RecordedEvent> {
        self.events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Returns every settled history line in the order the runtime reported it.
    pub(crate) fn settled(&self) -> Vec<(SessionId, HistoryLine)> {
        self.settled
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl RuntimeEvents for RecordedEvents {
    fn session_title_updated(&self, session_id: &SessionId) {
        self.events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(RecordedEvent::SessionTitleUpdated(session_id.clone()));
    }

    fn agent_models_invalidated(&self, agent_ref: &AgentRef) {
        self.events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(RecordedEvent::AgentModelsInvalidated(agent_ref.clone()));
    }

    fn records_settled(&self, session_id: &SessionId, lines: &[HistoryLine]) {
        self.settled
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend(lines.iter().map(|line| (session_id.clone(), line.clone())));
    }
}

/// Resolves every Workspace to one existing directory.
pub(crate) struct FixedDirectory {
    root: PathBuf,
}

impl WorkspaceDirectory for FixedDirectory {
    fn workspace_cwd(&self, _workspace_id: &WorkspaceId) -> Result<PathBuf, RuntimeError> {
        Ok(self.root.clone())
    }
}

/// Everything one test manager was built over, kept so the test can inspect it afterwards.
pub(crate) struct TestRuntime {
    pub manager: AgentRuntimeManager<TestHost>,
    pub events: RecordedEvents,
}

/// Builds a manager rooted at `root` with `installed` agent plugins over `store`.
pub(crate) fn test_runtime(
    root: &Path,
    installed: Vec<PluginId>,
    store: MemorySessionStore,
    scheduler: Scheduler,
) -> TestRuntime {
    let events = RecordedEvents::default();
    let manager = AgentRuntimeManager::new(AgentRuntimeSetup::<TestHost> {
        attach: Arc::new(InstalledAgents::new(installed)),
        store,
        directory: FixedDirectory {
            root: root.to_path_buf(),
        },
        session_setup: EmptySessionSetup::default(),
        events: events.clone(),
        home_directory: root.to_path_buf(),
        sessions_root: root.join("sessions"),
        scheduler,
    })
    .expect("build agent runtime manager");
    TestRuntime { manager, events }
}
