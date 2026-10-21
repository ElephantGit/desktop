//! Collection and retention orchestration for Scheduled Run artifacts.

use std::path::Path;
use std::sync::Arc;

use ora_application::{
    Clock, CollectScheduledRunArtifactsHandler, ScheduledArtifactStore,
    SweepScheduledRunArtifactsHandler,
};
use ora_db::{RepositoryPool, SqliteScheduledRunRepository};
use ora_domain::ScheduledRunId;
use ora_logging::ora_warn;

use crate::clock::SystemClock;

/// Owns the process-wide collection and sweep handlers for Scheduled Run artifacts.
#[derive(Clone)]
pub(super) struct ScheduledRunArtifactRuntime {
    collect: Arc<CollectScheduledRunArtifactsHandler<SqliteScheduledRunRepository>>,
    sweep: Arc<SweepScheduledRunArtifactsHandler<SqliteScheduledRunRepository>>,
    clock: SystemClock,
}

impl ScheduledRunArtifactRuntime {
    /// Builds both handlers over one repository and the shared artifact store.
    pub(super) fn new(
        pool: RepositoryPool,
        store: ScheduledArtifactStore,
        clock: SystemClock,
    ) -> Self {
        let repository = Arc::new(SqliteScheduledRunRepository::new(pool));
        Self {
            collect: Arc::new(CollectScheduledRunArtifactsHandler::new(
                repository.clone(),
                store.clone(),
            )),
            sweep: Arc::new(SweepScheduledRunArtifactsHandler::new(repository, store)),
            clock,
        }
    }

    /// Copies the run's declared artifacts before the janitor may discard the worktree.
    ///
    /// Collection is best-effort: a failure or partial rejection never rewrites the run's
    /// execution facts, and the rejected list remains visible from history.
    pub(super) fn collect(&self, run_id: &ScheduledRunId, worktree_path: &Path) {
        match self
            .collect
            .handle(run_id, worktree_path, self.clock.now_timestamp_millis())
        {
            Ok(collected) if !collected.rejections.is_empty() => {
                ora_warn!(
                    run_id = %run_id,
                    rejected = collected.rejections.len(),
                    "scheduled run artifacts were partially collected"
                );
            }
            Ok(_) => {}
            Err(error) => {
                ora_warn!(run_id = %run_id, error = %error, "scheduled run artifact collection failed");
            }
        }
    }

    /// Enforces the per-task artifact bound and removes cleanup left by an interrupted run.
    pub(super) fn sweep(&self) {
        if let Err(error) = self.sweep.handle() {
            ora_warn!(error = %error, "scheduled run artifact sweep failed");
        }
    }
}
