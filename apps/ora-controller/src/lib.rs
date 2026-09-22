//! Local durable clone coordination; no Desktop/Backend writer or Cloud authority is installed.
//! Coordination logic reaches persistence only through [`CoordinationStore`]; the SQLite adapter
//! under `sqlite` is the only implementation today.
#[cfg(target_os = "linux")]
mod api;
mod coordination;
#[cfg(target_os = "linux")]
mod deployment;
#[cfg(target_os = "linux")]
mod runtime;
#[cfg(target_os = "linux")]
mod service;
#[cfg(target_os = "linux")]
mod session;
#[cfg(target_os = "linux")]
mod single_node;
mod sqlite;
mod store;
#[cfg(target_os = "linux")]
mod transport;
#[cfg(target_os = "linux")]
pub use coordination::take_over;
pub use deployment::{ApiConfig, DeploymentConfig, NodeHosting, SingleNodeConfig};
use ora_node_protocol::*;
#[cfg(target_os = "linux")]
pub use runtime::{ControllerHandle, ControllerRuntime, Persistence, RuntimeConfig};
#[cfg(target_os = "linux")]
pub use service::Service;
#[cfg(target_os = "linux")]
pub use session::{NodeEndpoint, SessionConfig, run_session};
pub use sqlite::SqliteStore;
use std::path::PathBuf;
pub use store::CoordinationStore;
#[cfg(target_os = "linux")]
pub use transport::{DEFAULT_PORT, Listener, Transport};

/// Local persistence failures never authorize dispatch or acknowledgement.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("controller I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("controller storage: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("controller encoding: {0}")]
    Encoding(#[from] serde_json::Error),
    #[error("invalid message: {0}")]
    Validation(#[from] MessageValidationError),
    #[error("another runtime owns the Controller database")]
    AlreadyRunning,
    #[error("unrecognized Controller schema or identity")]
    InvalidStorage,
    #[error("input, result or dispatch ownership conflict")]
    Conflict,
    #[error("injected persistence failure")]
    Injected,
    #[error("invalid deployment composition: {0}")]
    Configuration(String),
}

/// Test seams refuse writes before transactions commit, using the same real SQLite and reconciliation.
/// Guards travel with the store onto the blocking pool, hence the thread-safety bounds.
pub trait WriteGuard: Send + 'static {
    /// Prevents a durable boundary; callers must not dispatch or acknowledge on failure.
    fn before_write(&self, point: WritePoint) -> Result<(), Error>;
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WritePoint {
    Accept,
    Takeover,
    Receipt,
    /// Final takeover boundary inside the open transaction, before SQLite commit or any Ack.
    Commit,
}
pub struct DurableWrites;
impl WriteGuard for DurableWrites {
    /// Production delegates all durability failures to SQLite.
    fn before_write(&self, _point: WritePoint) -> Result<(), Error> {
        Ok(())
    }
}

/// An accepted operation and its durable terminal fact; no result means awaiting reconciliation, not failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloneOperation {
    pub command: CloneRepositoryMessage,
    pub result: Option<CloneExecutionResult>,
}
