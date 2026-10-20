//! Restoring a session's prior Revision into its checkout before the agent starts (Node delivery
//! push ADR D5, D6; Node clone ADR D9).
//!
//! The Node fetches the prior delivery ref from the Git Server into a transport repository that
//! borrows the checkout's objects, negotiating from the clone's base commit so only what the
//! checkout lacks is sent, and checks that it is the prior final commit. The workload user then
//! takes the objects from the transport repository into the checkout and moves the Workspace
//! branch there. Everything runs on a blocking thread; the transport repository is removed when
//! the restore ends, whatever the outcome. One absolute deadline, owned by the deployment, bounds
//! the fetch and every workload Git command; neither a retry nor a wait grants more time.

mod config;
mod git;
mod runner;

pub use config::RestoreConfig;
pub(crate) use runner::RestoreGitRunner;

use super::snapshot::DeliveryGit;
use crate::git_server::{GitServerAccess, GitServerFailure, TimeLimit, TransportRepository};
use crate::session::{
    PriorRevisionRestore, RestoreCancel, RestoreFailure, RestoreRequest, Restored,
};
use git::GitRestore;
use gitlancer::GitRunner;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// Kind of the transport repositories restores use.
const TRANSPORT_KIND: &str = "restore";

/// One restore's execution timing; cleanup keeps its independent managed-process budget.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RestorePolicy {
    /// One absolute budget shared by the Git Server fetch and all workload Git steps.
    pub(crate) deadline: Duration,
}

impl RestorePolicy {
    /// The approved default: five minutes, shared from the first fetch exchange.
    pub(crate) const DEFAULT: Self = Self {
        deadline: Duration::from_millis(config::DEFAULT_DEADLINE_MS),
    };
}

/// The workload user's Git and the Node identity's Git a restore runs.
type RestoreGit<R, N> = (DeliveryGit<R>, N);

/// The production restore: Git Server access through `N` as the Node, checkout Git through `R`
/// with delivery's policy as the workload user.
pub(crate) struct RevisionRestorer<R: Send + 'static, N: Send + 'static> {
    /// One restore's Git at a time: a host-backed runner drives its own runtime, which concurrent
    /// blocking threads must not share.
    /// Taken out only when the restorer is dropped.
    git: Arc<Mutex<Option<RestoreGit<R, N>>>>,
    access: GitServerAccess,
    /// The clone repository root, whose transport directory holds the restore's repository.
    repository_root: PathBuf,
    /// The workload user restore Git runs as, when the deployment separates it from the Node.
    owner: Option<u32>,
    /// Ends the waits between Git Server attempts when the Node stops.
    shutdown: crate::Shutdown,
    policy: RestorePolicy,
}

impl<R: Send + 'static, N: Send + 'static> RevisionRestorer<R, N> {
    /// Composes one restorer for the Node's lifetime; only the deployment chooses its deadline.
    pub(crate) fn new(
        git: DeliveryGit<R>,
        node: N,
        access: GitServerAccess,
        repository_root: PathBuf,
        owner: Option<u32>,
        shutdown: crate::Shutdown,
        policy: RestorePolicy,
    ) -> Self {
        Self {
            git: Arc::new(Mutex::new(Some((git, node)))),
            access,
            repository_root,
            owner,
            shutdown,
            policy,
        }
    }

    /// The same restorer with a shorter deadline, for tests that run it out.
    #[cfg(test)]
    pub(super) fn within(mut self, limit: Duration) -> Self {
        self.policy.deadline = limit;
        self
    }
}

impl<R: Send + 'static, N: Send + 'static> Drop for RevisionRestorer<R, N> {
    /// Host-backed runners own synchronous runtimes, and the last session handle may be released
    /// on an async worker; those runtimes are retired on a joined thread that may block.
    fn drop(&mut self) {
        let git = self
            .git
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(git) = git {
            let _ = std::thread::spawn(move || drop(git)).join();
        }
    }
}

impl<R, N> PriorRevisionRestore for RevisionRestorer<R, N>
where
    R: RestoreGitRunner + Send + 'static,
    N: GitRunner + Send + 'static,
{
    /// Fetches, verifies and checks out the prior final commit on a blocking thread, which
    /// starts no further Git once the Node stops, the session is cancelled or the deadline ends.
    async fn restore(
        &self,
        request: RestoreRequest,
        cancel: RestoreCancel,
    ) -> Result<Restored, RestoreFailure> {
        if self.policy.deadline.is_zero() {
            return Err(RestoreFailure::Unavailable);
        }
        let deadline = Instant::now()
            .checked_add(self.policy.deadline)
            .ok_or(RestoreFailure::Unavailable)?;
        let git = Arc::clone(&self.git);
        let access = self.access.limited(TimeLimit::Until(deadline));
        let root = self.repository_root.clone();
        let owner = self.owner;
        let shutdown = self.shutdown.clone();
        tokio::task::spawn_blocking(move || {
            let stopping = || shutdown.requested() || cancel.requested();
            let git = git.lock().unwrap_or_else(PoisonError::into_inner);
            let Some((workload, node)) = git.as_ref() else {
                return Err(RestoreFailure::Unavailable);
            };
            let execution = request.execution.as_str();
            let transport =
                TransportRepository::create(node, &root, TRANSPORT_KIND, &request.execution)
                    .and_then(|transport| {
                        transport.borrow_objects(&request.checkout.join(".git"))?;
                        transport.negotiate_from(node, &[&request.base_commit])?;
                        Ok(transport)
                    })
                    .map_err(|error| {
                        ora_logging::ora_warn!(execution_id = %execution, error = %error, "restore transport repository could not be prepared");
                        RestoreFailure::Unavailable
                    })?;
            let prior = &request.prior;
            access
                .fetch(
                    node,
                    &transport,
                    &prior.git_repository_url,
                    &request.execution,
                    &[format!("{}:refs/ora/prior", prior.delivery_ref.as_str())],
                    &stopping,
                )
                .map_err(|failure| {
                    ora_logging::ora_warn!(execution_id = %execution, failure = ?failure, "prior delivery ref was not fetched");
                    match failure {
                        // The Git Server answers missing and unreadable refs the same way: the
                        // prior Revision's Git content is permanently out of reach.
                        GitServerFailure::Denied | GitServerFailure::MissingRef => {
                            RestoreFailure::BaseUnavailable
                        }
                        // A 401 only says the Node's own git-access material is wrong or
                        // expired; it must not mark the prior Revision unresumable, so a later
                        // session may try again once the deployment is fixed.
                        GitServerFailure::Unauthenticated
                        | GitServerFailure::Unavailable
                        | GitServerFailure::Stopped => RestoreFailure::Unavailable,
                    }
                })?;
            // A delivery ref only ever gets created, so another commit is a defect; it stays
            // retryable instead of making Cloud give up on the Revision.
            match transport.commit_of(node, "refs/ora/prior") {
                Ok(Some(commit)) if commit == prior.final_commit => {}
                Ok(_) | Err(_) => {
                    ora_logging::ora_warn!(execution_id = %execution, "prior delivery ref does not hold the prior final commit");
                    return Err(RestoreFailure::Unavailable);
                }
            }
            if stopping() {
                return Err(RestoreFailure::Unavailable);
            }
            transport.hand_over(owner).map_err(|error| {
                ora_logging::ora_warn!(execution_id = %execution, error = %error, "restore transport repository could not be handed over");
                RestoreFailure::Unavailable
            })?;
            workload.restore(&GitRestore {
                checkout: &request.checkout,
                transport: transport.path(),
                prior,
                branch: &request.branch,
                base_commit: &request.base_commit,
                deadline,
                stopping: &stopping,
            })
        })
        .await
        .unwrap_or(Err(RestoreFailure::Unavailable))
    }
}
