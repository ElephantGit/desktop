use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use uuid::Uuid;

use crate::{CleanupState, DirectProcessState, OutputPolicy};

/// A single launch attempt, independent of OS process and Node instance identities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RunId(Uuid);

impl RunId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for RunId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for RunId {
    /// Formats the stable identity without exposing a process identifier.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

/// The policy chosen before the direct process can leave descendants behind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DescendantPolicy {
    Cleanup { grace: Duration },
    WaitForAll,
}

/// An exact local launch specification; command normalization and wire encoding remain separate.
///
/// OS strings preserve non-UTF-8 inputs. Nothing in this type persists environment values or secrets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSpec {
    pub program: OsString,
    pub args: Vec<OsString>,
    pub cwd: PathBuf,
    pub env: BTreeMap<OsString, OsString>,
    pub descendants: DescendantPolicy,
    pub output: OutputPolicy,
}

impl RunSpec {
    /// Requires an explicit exit policy so this layer does not invent a deployment's grace period.
    pub fn new(
        program: impl Into<OsString>,
        cwd: impl Into<PathBuf>,
        descendants: DescendantPolicy,
    ) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            cwd: cwd.into(),
            env: BTreeMap::new(),
            descendants,
            output: OutputPolicy::Discard,
        }
    }
}

/// What is known about whether this attempt ever executed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchFact {
    NotStarted(String),
    Started,
    Unknown(String),
}

/// Facts exposed to the guardian's caller, not an acknowledgement of durable acceptance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSnapshot {
    pub id: RunId,
    pub launch: LaunchFact,
    pub direct: DirectProcessState,
    pub cleanup: CleanupState,
}
