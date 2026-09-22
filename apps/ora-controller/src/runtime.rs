use super::*;
use serde::{Deserialize, Serialize};
use std::{future::Future, io, sync::Arc, time::Duration};

/// Which authority persists coordination for this deployment. Chosen once at deployment time: a
/// running Controller never switches adapters, and neither adapter is a fallback for the other,
/// because two authorities would leave nobody able to say which record is the fact.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Persistence {
    /// Local single-node deployments: the SQLite database and its lease live in `home_directory`.
    Sqlite,
    /// Cloud deployments: every durable operation is a call to the Cloud internal control contract
    /// at `endpoint`; no database is opened locally. The adapter itself lands in a later change.
    Cloud { endpoint: String },
}

/// Shared deployment configuration for the standalone executable and embedded HTTP composition.
/// `home_directory` is the process-private state root in both modes (API socket, and in SQLite
/// mode the database); it never holds cloud-authoritative records.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeConfig {
    pub home_directory: PathBuf,
    pub persistence: Persistence,
    pub protected_state_directories: Vec<PathBuf>,
    pub controller_id: ControllerId,
    pub nodes: Vec<NodeEndpoint>,
    pub session: SessionConfig,
    pub reconnect_ms: u64,
    pub timezone: String,
}

/// Owns deployment and the reconnect lifetime; callers supply their own process shutdown signal.
/// The store type is fixed at construction: one deployment runs exactly one persistence adapter.
pub struct ControllerRuntime<S: CoordinationStore> {
    handle: ControllerHandle<S>,
    config: RuntimeConfig,
}

/// Narrow application access to the durable store; the store itself decides how its work is executed.
pub struct ControllerHandle<S: CoordinationStore> {
    store: S,
    nodes: Arc<Vec<NodeId>>,
}

impl<S: CoordinationStore> Clone for ControllerHandle<S> {
    fn clone(&self) -> Self {
        Self {
            store: self.store.clone(),
            nodes: self.nodes.clone(),
        }
    }
}

impl ControllerRuntime<SqliteStore> {
    /// Validates deployment before opening local state, preserving protected roots and exclusive ownership.
    /// Only SQLite persistence can be opened here; a cloud deployment is refused before any state exists.
    pub fn open(config: RuntimeConfig) -> Result<Self, Error> {
        match &config.persistence {
            Persistence::Sqlite => {}
            Persistence::Cloud { endpoint } => {
                return Err(Error::Configuration(format!(
                    "cloud persistence at {endpoint} is not available in this build; use sqlite"
                )));
            }
        }
        if !config.home_directory.is_absolute()
            || config.reconnect_ms == 0
            || config.session.query_interval_ms == 0
            || config.session.io_timeout_ms == 0
        {
            return Err(Error::InvalidStorage);
        }
        for (index, node) in config.nodes.iter().enumerate() {
            if node.node_id.as_str().trim().is_empty()
                || !node.endpoint.is_absolute()
                || config.nodes[..index]
                    .iter()
                    .any(|other| other.node_id == node.node_id || other.endpoint == node.endpoint)
            {
                return Err(Error::Conflict);
            }
        }
        let home = ora_utils::path::canonicalize_longest_existing_prefix(&config.home_directory);
        for root in config
            .protected_state_directories
            .iter()
            .map(PathBuf::as_path)
            .chain(
                config
                    .nodes
                    .iter()
                    .filter_map(|node| node.endpoint.parent()),
            )
        {
            if !root.is_absolute() {
                return Err(Error::InvalidStorage);
            }
            let root = ora_utils::path::canonicalize_longest_existing_prefix(root);
            if home.starts_with(&root) || root.starts_with(&home) {
                return Err(Error::InvalidStorage);
            }
        }
        let store = SqliteStore::open(&config.home_directory, config.controller_id.clone())?;
        Ok(Self::with_store(config, store))
    }
}

impl<S: CoordinationStore> ControllerRuntime<S> {
    /// Binds validated deployment to an already opened store; adapters validate their own state.
    fn with_store(config: RuntimeConfig, store: S) -> Self {
        let nodes = Arc::new(
            config
                .nodes
                .iter()
                .map(|node| node.node_id.clone())
                .collect(),
        );
        Self {
            handle: ControllerHandle { store, nodes },
            config,
        }
    }

    /// Supplies application access without exposing the store, mutex or reconnect implementation.
    pub fn handle(&self) -> ControllerHandle<S> {
        self.handle.clone()
    }

    /// Reconnects configured Nodes until shutdown; cancellation drops the JoinSet and aborts every session.
    pub async fn run(&self, shutdown: impl Future<Output = ()>) -> io::Result<()> {
        let mut sessions = tokio::task::JoinSet::new();
        for target in self.config.nodes.clone() {
            let store = self.handle.store.clone();
            let settings = self.config.session.clone();
            let delay = Duration::from_millis(self.config.reconnect_ms);
            sessions.spawn(async move {
                loop {
                    if run_session(&store, &target, &settings).await.is_err() { ora_logging::ora_warn!(node_id = %target.node_id.as_str(), "Controller connection unavailable; original execution responsibility retained"); }
                    tokio::time::sleep(delay).await;
                }
            });
        }
        ora_logging::ora_info!("Controller recovery started");
        let result = tokio::select! {
            _ = shutdown => Ok(()),
            result = sessions.join_next(), if !sessions.is_empty() => Err(io::Error::other(format!("Controller session task stopped: {result:?}"))),
        };
        sessions.abort_all();
        while sessions.join_next().await.is_some() {}
        result
    }
}

impl<S: CoordinationStore> ControllerHandle<S> {
    /// Accepts only a deployment-configured target before any Node dispatch observes the operation.
    pub async fn accept_clone(
        &self,
        request: RequestId,
        spec: CloneExecutionSpec,
    ) -> Result<CloneRepositoryMessage, Error> {
        if !self.nodes.contains(&spec.node_id) {
            return Err(Error::Conflict);
        }
        self.store.accept_request(request, spec).await
    }

    /// Returns accepted operations, including pending responsibility while Nodes are disconnected.
    pub async fn operations(&self) -> Result<Vec<CloneOperation>, Error> {
        self.store.operations().await
    }

    /// Reads one operation without confusing missing identity with an unknown terminal result.
    pub async fn operation(&self, execution: ExecutionId) -> Result<Option<CloneOperation>, Error> {
        self.store.operation(&execution).await
    }
}
