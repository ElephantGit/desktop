//! In-process Node lifecycle; transport and process bootstrapping are separate concerns.
use ora_node_db::{NodeDatabase, NodeIdentity};
use ora_node_protocol::{
    MainWorkspaceBinding, NodeIncarnationId, NodeRuntimeIdentity, RepositoryRef,
};
use std::path::{Path, PathBuf};

/// One explicitly registered repository and its existing Main Workspace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepositoryBinding {
    pub repository: RepositoryRef,
    pub main_workspace: MainWorkspaceBinding,
    pub authorized_root: PathBuf,
    pub worktree_root: PathBuf,
}

/// Deployment supplies `~/.ora/node`; tests inject an isolated temporary directory.
pub struct NodeConfig {
    pub home_directory: PathBuf,
    pub identity: NodeIdentity,
    pub repositories: Vec<RepositoryBinding>,
}

/// Owns Node-local state and the database execution lease for its entire lifetime.
pub struct Node {
    home_directory: PathBuf,
    database: NodeDatabase,
    identity: NodeRuntimeIdentity,
    repositories: Vec<RepositoryBinding>,
}

impl Node {
    /// Initializes an isolated Node without starting IPC or performing Git mutations.
    pub fn open(config: NodeConfig) -> Result<Self, ora_node_db::Error> {
        std::fs::create_dir_all(&config.home_directory)?;
        let database = NodeDatabase::open(
            &config.home_directory.join("ora-node.sqlite3"),
            config.identity,
        )?;
        let identity = NodeRuntimeIdentity {
            node_id: database.node_id().clone(),
            incarnation_id: NodeIncarnationId::new(uuid::Uuid::new_v4().to_string()),
        };
        Ok(Self {
            home_directory: config.home_directory,
            database,
            identity,
            repositories: config.repositories,
        })
    }

    /// Returns the injected state root, including the database and future Node runtime files.
    pub fn home_directory(&self) -> &Path {
        &self.home_directory
    }

    /// Returns the stable Node identity and this process's fresh incarnation.
    pub fn identity(&self) -> &NodeRuntimeIdentity {
        &self.identity
    }

    /// Returns explicit repository registrations for inspection without granting mutation access.
    pub fn repositories(&self) -> &[RepositoryBinding] {
        &self.repositories
    }

    /// Returns persistent identity as recorded by the leased database.
    pub fn node_id(&self) -> &ora_node_protocol::NodeId {
        self.database.node_id()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use pretty_assertions::assert_eq;

    /// A temporary home models deployment without depending on the process HOME variable.
    #[test]
    fn injected_home_persists_node_but_not_incarnation() {
        let dir = tempfile::tempdir().unwrap();
        let config = || NodeConfig {
            home_directory: dir.path().to_path_buf(),
            identity: NodeIdentity::Discover,
            repositories: vec![],
        };
        let first = Node::open(config()).unwrap();
        assert_eq!(first.home_directory(), dir.path());
        assert!(dir.path().join("ora-node.sqlite3").is_file());
        let identity = first.identity().clone();
        drop(first);
        let next = Node::open(config()).unwrap();
        assert_eq!(next.node_id(), &identity.node_id);
        assert_ne!(next.identity().incarnation_id, identity.incarnation_id);
    }
}
