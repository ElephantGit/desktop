use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use ora_domain::{
    MAX_SCHEDULED_ARTIFACT_RUNS_PER_TASK, SCHEDULED_ARTIFACT_DIRECTORY, ScheduledArtifactRejection,
    ScheduledArtifactRejectionReason, ScheduledRunArtifact, ScheduledRunId, ScheduledTaskId,
};

use super::artifact_store::{
    CollectedScheduledArtifacts, ScheduledArtifactStore, ScheduledArtifactStoreError,
};
use super::{ScheduledArtifactRunRecord, ScheduledRunArtifactRepository};
use crate::ApplicationError;

/// Collects and registers the artifacts one succeeded run declared in its worktree.
pub struct CollectScheduledRunArtifactsHandler<Repository> {
    repository: Arc<Repository>,
    store: ScheduledArtifactStore,
}

impl<Repository> CollectScheduledRunArtifactsHandler<Repository> {
    pub fn new(repository: Arc<Repository>, store: ScheduledArtifactStore) -> Self {
        Self { repository, store }
    }
}

impl<Repository> CollectScheduledRunArtifactsHandler<Repository>
where
    Repository: ScheduledRunArtifactRepository + Send + Sync + 'static,
{
    /// Copies accepted files under the fixed directory and records the manifest and rejections.
    ///
    /// The process-wide store lock keeps the sweep from observing the gap between a copied
    /// file and its manifest row. An unexpected storage failure is recorded as a rejected
    /// candidate so history still explains why nothing was collected; collection never
    /// rewrites the run's execution facts.
    pub fn handle(
        &self,
        run_id: &ScheduledRunId,
        worktree_path: &Path,
        registered_at: i64,
    ) -> Result<CollectedScheduledArtifacts, ApplicationError> {
        let _guard = self.store.lock();
        let collected = match self.store.collect(run_id, worktree_path, registered_at) {
            Ok(collected) => collected,
            Err(ScheduledArtifactStoreError::Io(_)) => CollectedScheduledArtifacts {
                artifacts: Vec::new(),
                rejections: vec![ScheduledArtifactRejection {
                    name: SCHEDULED_ARTIFACT_DIRECTORY.to_string(),
                    reason: ScheduledArtifactRejectionReason::ReadFailed,
                }],
            },
            Err(
                error @ (ScheduledArtifactStoreError::InvalidRunId
                | ScheduledArtifactStoreError::UnsafeArtifactPath
                | ScheduledArtifactStoreError::ArtifactChanged),
            ) => {
                return Err(artifact_store_error(error));
            }
        };
        self.repository
            .record_scheduled_run_artifacts(
                run_id,
                &collected.artifacts,
                &collected.rejections,
                registered_at,
            )
            .map_err(ApplicationError::from_scheduled_run_artifact_repository_error)?;
        Ok(collected)
    }
}

/// Reconciles artifact storage with the manifest and enforces the per-task retention bound.
pub struct SweepScheduledRunArtifactsHandler<Repository> {
    repository: Arc<Repository>,
    store: ScheduledArtifactStore,
}

impl<Repository> SweepScheduledRunArtifactsHandler<Repository> {
    pub fn new(repository: Arc<Repository>, store: ScheduledArtifactStore) -> Self {
        Self { repository, store }
    }
}

impl<Repository> SweepScheduledRunArtifactsHandler<Repository>
where
    Repository: ScheduledRunArtifactRepository + Send + Sync + 'static,
{
    /// Removes orphan rows and files, then keeps at most ten finished runs per task.
    ///
    /// Runs that still retain their worktree are exempt from the bound and never counted, so
    /// clearing the retained-worktree link makes the run's artifacts eligible again. Soft-
    /// deleted tasks lose every run's artifacts; disabled tasks keep theirs because the task
    /// may be re-enabled at any time.
    pub fn handle(&self) -> Result<(), ApplicationError> {
        let _guard = self.store.lock();
        let records = self
            .repository
            .list_scheduled_artifact_runs()
            .map_err(ApplicationError::from_scheduled_run_artifact_repository_error)?;
        let mut live_runs = HashSet::new();
        let mut retention: HashMap<ScheduledTaskId, Vec<ScheduledArtifactRunRecord>> =
            HashMap::new();
        for record in records {
            if record.run_missing || record.task_deleted {
                self.repository
                    .delete_scheduled_run_artifact_rows(&record.run_id)
                    .map_err(ApplicationError::from_scheduled_run_artifact_repository_error)?;
                remove_run_storage(&self.store, &record.run_id);
                continue;
            }
            live_runs.insert(record.run_id.to_string());
            let artifacts = self
                .repository
                .list_scheduled_run_artifacts(&record.run_id)
                .map_err(ApplicationError::from_scheduled_run_artifact_repository_error)?;
            let mismatched = self
                .store
                .reconcile_run(&record.run_id, &artifacts)
                .map_err(artifact_store_error)?;
            let remaining = artifacts.len().saturating_sub(mismatched.len());
            for relative_path in mismatched {
                self.repository
                    .delete_scheduled_run_artifact(&record.run_id, &relative_path)
                    .map_err(ApplicationError::from_scheduled_run_artifact_repository_error)?;
            }
            if remaining == 0 || record.retains_worktree {
                continue;
            }
            if let Some(task_id) = record.task_id.clone() {
                retention.entry(task_id).or_default().push(record);
            }
        }

        for runs in retention.values_mut() {
            runs.sort_by_key(|record| {
                (record.updated_at.unwrap_or(i64::MIN), record.run_id.clone())
            });
            let excess = runs
                .len()
                .saturating_sub(MAX_SCHEDULED_ARTIFACT_RUNS_PER_TASK);
            for record in runs.iter().take(excess) {
                self.repository
                    .delete_scheduled_run_artifact_rows(&record.run_id)
                    .map_err(ApplicationError::from_scheduled_run_artifact_repository_error)?;
                remove_run_storage(&self.store, &record.run_id);
                live_runs.remove(record.run_id.as_ref());
            }
        }

        self.store
            .remove_run_directories_not_in(&live_runs)
            .map_err(artifact_store_error)?;
        Ok(())
    }
}

/// Lists one visible run's collected artifacts and rejected candidates.
pub struct ListScheduledRunArtifactsHandler<Repository> {
    repository: Arc<Repository>,
}

impl<Repository> ListScheduledRunArtifactsHandler<Repository> {
    pub fn new(repository: Arc<Repository>) -> Self {
        Self { repository }
    }
}

impl<Repository> ListScheduledRunArtifactsHandler<Repository>
where
    Repository: ScheduledRunArtifactRepository + Send + Sync + 'static,
{
    /// Returns the manifest and rejected-candidate log for one run.
    ///
    /// A pruned run or a soft-deleted task reports "run not found", so artifact access always
    /// follows the run's visibility. An existing run with no artifacts returns empty lists.
    pub fn handle(
        &self,
        run_id: &ScheduledRunId,
    ) -> Result<(Vec<ScheduledRunArtifact>, Vec<ScheduledArtifactRejection>), ApplicationError>
    {
        require_visible_run(self.repository.as_ref(), run_id)?;
        let artifacts = self
            .repository
            .list_scheduled_run_artifacts(run_id)
            .map_err(ApplicationError::from_scheduled_run_artifact_repository_error)?;
        let rejections = self
            .repository
            .list_scheduled_run_artifact_rejections(run_id)
            .map_err(ApplicationError::from_scheduled_run_artifact_repository_error)?;
        Ok((artifacts, rejections))
    }
}

/// Reads one artifact's bytes for an in-app preview.
pub struct ReadScheduledRunArtifactHandler<Repository> {
    repository: Arc<Repository>,
    store: ScheduledArtifactStore,
}

impl<Repository> ReadScheduledRunArtifactHandler<Repository> {
    pub fn new(repository: Arc<Repository>, store: ScheduledArtifactStore) -> Self {
        Self { repository, store }
    }
}

impl<Repository> ReadScheduledRunArtifactHandler<Repository>
where
    Repository: ScheduledRunArtifactRepository + Send + Sync + 'static,
{
    /// Loads one registered artifact after rechecking its run visibility and stored size.
    pub fn handle(
        &self,
        run_id: &ScheduledRunId,
        relative_path: &str,
    ) -> Result<Vec<u8>, ApplicationError> {
        let _guard = self.store.lock();
        require_visible_run(self.repository.as_ref(), run_id)?;
        let artifact = self
            .repository
            .find_scheduled_run_artifact(run_id, relative_path)
            .map_err(ApplicationError::from_scheduled_run_artifact_repository_error)?
            .ok_or_else(|| artifact_not_found(run_id, relative_path))?;
        self.store
            .read_artifact(run_id, &artifact)
            .map_err(|error| read_error(error, run_id, relative_path))
    }
}

/// Copies one registered artifact to a user-selected destination.
pub struct CopyScheduledRunArtifactHandler<Repository> {
    repository: Arc<Repository>,
    store: ScheduledArtifactStore,
}

impl<Repository> CopyScheduledRunArtifactHandler<Repository> {
    pub fn new(repository: Arc<Repository>, store: ScheduledArtifactStore) -> Self {
        Self { repository, store }
    }
}

impl<Repository> CopyScheduledRunArtifactHandler<Repository>
where
    Repository: ScheduledRunArtifactRepository + Send + Sync + 'static,
{
    /// Writes one artifact to `destination`, the path the native save dialog returned.
    pub fn handle(
        &self,
        run_id: &ScheduledRunId,
        relative_path: &str,
        destination: &Path,
    ) -> Result<(), ApplicationError> {
        let _guard = self.store.lock();
        require_visible_run(self.repository.as_ref(), run_id)?;
        let artifact = self
            .repository
            .find_scheduled_run_artifact(run_id, relative_path)
            .map_err(ApplicationError::from_scheduled_run_artifact_repository_error)?
            .ok_or_else(|| artifact_not_found(run_id, relative_path))?;
        let source = self
            .store
            .artifact_path(run_id, &artifact)
            .map_err(|error| read_error(error, run_id, relative_path))?;
        std::fs::copy(source, destination).map_err(|source| ApplicationError::TaskFilesystem {
            context: "failed to export a scheduled run artifact",
            source,
        })?;
        Ok(())
    }
}

/// Rejects a pruned run or soft-deleted task before any manifest row is read.
fn require_visible_run<Repository>(
    repository: &Repository,
    run_id: &ScheduledRunId,
) -> Result<(), ApplicationError>
where
    Repository: ScheduledRunArtifactRepository,
{
    let subject = repository
        .find_scheduled_artifact_run(run_id)
        .map_err(ApplicationError::from_scheduled_run_artifact_repository_error)?
        .ok_or_else(|| ApplicationError::ScheduledRunNotFound {
            run_id: run_id.to_string(),
        })?;
    if subject.run_missing || subject.task_deleted {
        return Err(ApplicationError::ScheduledRunNotFound {
            run_id: run_id.to_string(),
        });
    }
    Ok(())
}

/// Builds the stable not-found error for one run-relative artifact path.
fn artifact_not_found(run_id: &ScheduledRunId, relative_path: &str) -> ApplicationError {
    ApplicationError::ScheduledRunArtifactNotFound {
        run_id: run_id.to_string(),
        relative_path: relative_path.to_string(),
    }
}

/// Maps a storage read failure to not-found for vanished/unsafe files and internal otherwise.
fn read_error(
    error: super::artifact_store::ScheduledArtifactStoreError,
    run_id: &ScheduledRunId,
    relative_path: &str,
) -> ApplicationError {
    match error {
        super::artifact_store::ScheduledArtifactStoreError::Io(ref source)
            if source.kind() == std::io::ErrorKind::NotFound =>
        {
            artifact_not_found(run_id, relative_path)
        }
        super::artifact_store::ScheduledArtifactStoreError::ArtifactChanged
        | super::artifact_store::ScheduledArtifactStoreError::UnsafeArtifactPath
        | super::artifact_store::ScheduledArtifactStoreError::InvalidRunId => {
            artifact_not_found(run_id, relative_path)
        }
        _ => ApplicationError::ScheduledRunArtifactStorage {
            context: "failed to read a scheduled run artifact",
        },
    }
}

/// Maps store failures that are not tied to one readable artifact.
fn artifact_store_error(
    error: super::artifact_store::ScheduledArtifactStoreError,
) -> ApplicationError {
    ApplicationError::ScheduledRunArtifactStorage {
        context: match error {
            super::artifact_store::ScheduledArtifactStoreError::InvalidRunId => {
                "scheduled run id is not a safe storage segment"
            }
            super::artifact_store::ScheduledArtifactStoreError::UnsafeArtifactPath => {
                "artifact path is not safe to materialize"
            }
            super::artifact_store::ScheduledArtifactStoreError::ArtifactChanged => {
                "artifact no longer matches its registered size"
            }
            super::artifact_store::ScheduledArtifactStoreError::Io(_) => {
                "artifact storage operation failed"
            }
        },
    }
}

/// Removes one run's directory, logging rather than failing the whole sweep.
fn remove_run_storage(store: &ScheduledArtifactStore, run_id: &ScheduledRunId) {
    if let Err(error) = store.delete_run(run_id) {
        ora_logging::ora_warn!(
            run_id = %run_id,
            error = %error,
            "scheduled run artifact directory could not be removed"
        );
    }
}

#[cfg(test)]
mod tests;
