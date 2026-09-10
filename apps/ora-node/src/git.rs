use crate::resources::failure;
use gitlancer::git::{
    branch::{BranchDeletionMode, DeleteBranchRequest, ListBranchesRequest},
    repository::ListWorktreesRequest,
    worktree::{CreateWorktreeRequest, DeleteWorktreeRequest, WorktreeDeletionMode},
};
use gitlancer::{Git, GitRunner, RepoRoot, Repository, WorktreeKind};
use ora_node_db::Target;
use ora_node_protocol::{BranchName, CommitId, WorktreeFailure, WorktreeFailureCode};
use std::path::{Path, PathBuf};

/// Filesystem evidence is distinct from Git registration; neither alone proves success.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectoryState {
    Absent,
    Empty,
    Nonempty,
}

/// A validated linked checkout belongs to the expected repository and registered metadata directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Checkout {
    pub path: PathBuf,
    pub branch: BranchName,
    pub head: CommitId,
}

/// Observations keep independent Git and filesystem facts for conservative reconciliation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Observation {
    pub checkout: Option<Checkout>,
    pub branch: Option<CommitId>,
    pub directory: DirectoryState,
    pub branch_elsewhere: bool,
}
impl Observation {
    /// Reports complete absence, including the local task branch and its checkout registration.
    pub fn absent(&self) -> bool {
        self.checkout.is_none()
            && self.branch.is_none()
            && self.directory == DirectoryState::Absent
            && !self.branch_elsewhere
    }
}

/// Supplies Git and filesystem facts plus individually recoverable mutations.
/// Implementations must inspect authoritative registration and never compensate mutations implicitly.
pub trait WorktreeGit {
    /// Proves this is an existing main checkout and returns its canonical Git metadata directory.
    fn main_git_directory(&self, main: &Path) -> Result<PathBuf, WorktreeFailure>;
    /// Resolves an immutable base and validates the requested literal local branch name.
    fn resolve_base(
        &self,
        main: &Path,
        reference: &str,
        branch: &str,
    ) -> Result<CommitId, WorktreeFailure>;
    /// Returns independent registration, branch and filesystem facts without changing them.
    fn observe(&self, target: &Target) -> Result<Observation, WorktreeFailure>;
    /// Creates only the reserved worktree and branch at the frozen commit.
    fn create(&self, target: &Target) -> Result<(), WorktreeFailure>;
    /// Force-removes only a currently validated linked checkout owned by the execution.
    fn remove_worktree(&self, target: &Target) -> Result<(), WorktreeFailure>;
    /// Force-removes the owned local branch, respecting Git's other-worktree protection.
    fn remove_branch(&self, target: &Target) -> Result<(), WorktreeFailure>;
    /// Removes an owned empty directory; nonempty residual files must never be recursively removed.
    fn remove_empty_directory(&self, target: &Target) -> Result<(), WorktreeFailure>;
}

impl<R: GitRunner> WorktreeGit for Git<R> {
    /// Cross-checks repository discovery, main registration and the live checkout's metadata directory.
    fn main_git_directory(&self, main: &Path) -> Result<PathBuf, WorktreeFailure> {
        let repository = self
            .discover_repository(RepoRoot::new(main))
            .map_err(git_failure)?;
        if repository
            .root()
            .as_path()
            .canonicalize()
            .map_err(io_failure)?
            != main
        {
            return Err(failure(
                WorktreeFailureCode::InvalidMainWorkspace,
                "binding is not the main checkout",
            ));
        }
        let list = self
            .list_worktrees(ListWorktreesRequest {
                repository: &repository,
            })
            .map_err(git_failure)?;
        if !list.worktrees.iter().any(|w| {
            matches!(w.kind(), WorktreeKind::Main)
                && w.worktree_root().as_path().canonicalize().ok().as_deref() == Some(main)
        }) {
            return Err(failure(
                WorktreeFailureCode::InvalidMainWorkspace,
                "main checkout registration is missing",
            ));
        }
        let directory = self
            .checkout_git_directory(main)
            .map_err(git_failure)?
            .canonicalize()
            .map_err(io_failure)?;
        // Repository discovery already excluded linked checkouts; this also excludes bare roots.
        // A valid main checkout can keep its Git directory outside the checkout via a .git file.
        if self.checkout_root(main).map_err(git_failure)? != main {
            return Err(failure(
                WorktreeFailureCode::InvalidMainWorkspace,
                "main Git directory does not match",
            ));
        }
        Ok(directory)
    }

    /// Resolves a base once; recovery never resolves the mutable reference again.
    fn resolve_base(
        &self,
        main: &Path,
        reference: &str,
        branch: &str,
    ) -> Result<CommitId, WorktreeFailure> {
        let repository = Repository::new(RepoRoot::new(main));
        self.validate_branch_name(&repository, branch)
            .map_err(|e| failure(WorktreeFailureCode::BranchConflict, e.to_string()))?;
        let commit = self
            .resolve_commit(&repository, reference)
            .map_err(|e| failure(WorktreeFailureCode::BaseRefNotFound, e.to_string()))?;
        Ok(CommitId::new(commit.as_str()))
    }

    /// Verifies the registration and live checkout agree before exposing a deletable checkout fact.
    fn observe(&self, target: &Target) -> Result<Observation, WorktreeFailure> {
        let repository = Repository::new(RepoRoot::new(&target.main_path));
        let worktrees = self
            .list_worktrees(ListWorktreesRequest {
                repository: &repository,
            })
            .map_err(git_failure)?
            .worktrees;
        let mut checkout = None;
        let mut branch_elsewhere = false;
        for worktree in worktrees {
            let path = ora_utils::path::canonicalize_longest_existing_prefix(
                worktree.worktree_root().as_path(),
            );
            if path != target.path {
                if worktree
                    .branch_name()
                    .is_some_and(|b| b.as_str() == target.branch.as_str())
                {
                    branch_elsewhere = true;
                }
                if path.starts_with(&target.path) || target.path.starts_with(&path) {
                    return Err(failure(
                        WorktreeFailureCode::WorktreeConflict,
                        "target overlaps another checkout",
                    ));
                }
                continue;
            }
            if matches!(worktree.kind(), WorktreeKind::Main)
                || worktree.branch_name().map(gitlancer::BranchName::as_str)
                    != Some(target.branch.as_str())
            {
                return Err(failure(
                    WorktreeFailureCode::WorktreeConflict,
                    "registered target has different ownership facts",
                ));
            }
            self.verify_linked_checkout(&path, &target.git_directory)
                .map_err(git_failure)?;
            let head = self
                .resolve_commit(&Repository::new(RepoRoot::new(&path)), "HEAD")
                .map_err(git_failure)?;
            checkout = Some(Checkout {
                path,
                branch: target.branch.clone(),
                head: CommitId::new(head.as_str()),
            });
        }
        let branches = self
            .list_branches(ListBranchesRequest {
                repository: &repository,
            })
            .map_err(git_failure)?;
        let branch = if branches
            .branches
            .iter()
            .any(|b| b.as_str() == target.branch.as_str())
        {
            // A fully qualified ref avoids ambiguous tags with the same short name.
            Some(CommitId::new(
                self.resolve_commit(
                    &repository,
                    &format!("refs/heads/{}", target.branch.as_str()),
                )
                .map_err(git_failure)?
                .as_str(),
            ))
        } else {
            None
        };
        let directory = match std::fs::symlink_metadata(&target.path) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(failure(
                    WorktreeFailureCode::WorktreeConflict,
                    "target is a link or non-directory",
                ));
            }
            Ok(_) => {
                if std::fs::read_dir(&target.path)
                    .map_err(io_failure)?
                    .next()
                    .transpose()
                    .map_err(io_failure)?
                    .is_none()
                {
                    DirectoryState::Empty
                } else {
                    DirectoryState::Nonempty
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => DirectoryState::Absent,
            Err(error) => return Err(io_failure(error)),
        };
        Ok(Observation {
            checkout,
            branch,
            directory,
            branch_elsewhere,
        })
    }

    /// Leaves all partial effects intact so the persisted execution can reconcile them.
    fn create(&self, target: &Target) -> Result<(), WorktreeFailure> {
        self.create_worktree_for_recovery(CreateWorktreeRequest {
            repository: &Repository::new(RepoRoot::new(&target.main_path)),
            worktree_root: gitlancer::WorktreeRoot::new(&target.path),
            branch_name: gitlancer::BranchName::new(target.branch.as_str()),
            base_commit_id: gitlancer::CommitId::new(target.base_commit.as_str()),
        })
        .map_err(git_failure)
    }

    /// Re-reads registration immediately before using the typed force deletion API.
    fn remove_worktree(&self, target: &Target) -> Result<(), WorktreeFailure> {
        self.observe(target)?;
        let repository = Repository::new(RepoRoot::new(&target.main_path));
        let worktree = self
            .list_worktrees(ListWorktreesRequest {
                repository: &repository,
            })
            .map_err(git_failure)?
            .worktrees
            .into_iter()
            .find(|w| {
                w.worktree_root().as_path().canonicalize().ok().as_ref() == Some(&target.path)
            })
            .ok_or_else(|| {
                failure(
                    WorktreeFailureCode::WorktreeConflict,
                    "worktree registration disappeared",
                )
            })?;
        self.delete_worktree(DeleteWorktreeRequest {
            repository: &repository,
            worktree: &worktree,
            mode: WorktreeDeletionMode::Force,
        })
        .map_err(git_failure)?;
        Ok(())
    }

    /// Git refuses branch deletion if another checkout now uses it.
    fn remove_branch(&self, target: &Target) -> Result<(), WorktreeFailure> {
        self.delete_branch(DeleteBranchRequest {
            repository: &Repository::new(RepoRoot::new(&target.main_path)),
            branch_name: gitlancer::BranchName::new(target.branch.as_str()),
            mode: BranchDeletionMode::Force,
        })
        .map_err(git_failure)?;
        Ok(())
    }

    /// Nonrecursive removal fails closed if files appeared after the empty-directory observation.
    fn remove_empty_directory(&self, target: &Target) -> Result<(), WorktreeFailure> {
        std::fs::remove_dir(&target.path).map_err(io_failure)
    }
}

/// Preserves Git diagnostics while leaving terminal-versus-unknown policy to the execution owner.
fn git_failure(error: gitlancer::GitlancerError) -> WorktreeFailure {
    failure(WorktreeFailureCode::OperationFailed, error.to_string())
}
/// Preserves filesystem errors without conflating inaccessible paths with absent paths.
fn io_failure(error: std::io::Error) -> WorktreeFailure {
    failure(WorktreeFailureCode::OperationFailed, error.to_string())
}
