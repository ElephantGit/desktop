use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use ora_domain::{
    SCHEDULED_ARTIFACT_DIRECTORY, ScheduledArtifactRejection, ScheduledArtifactRejectionReason,
    ScheduledRunArtifact, ScheduledRunId,
};
use ora_utils::path::{StrictRelativePath, check_directory_without_symlinks};

mod collection;

/// One collection pass over the fixed worktree artifact directory.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CollectedScheduledArtifacts {
    pub artifacts: Vec<ScheduledRunArtifact>,
    pub rejections: Vec<ScheduledArtifactRejection>,
}

/// Reports why an artifact storage operation could not complete.
///
/// Collection treats every failure as best-effort and leaves the run facts untouched; the
/// sweep retries the same operation on the next startup or finished run.
#[derive(Debug, thiserror::Error)]
pub enum ScheduledArtifactStoreError {
    #[error("scheduled run id is not a safe single path segment")]
    InvalidRunId,
    #[error("artifact relative path is not safe to materialize")]
    UnsafeArtifactPath,
    #[error("artifact file no longer matches its registered size")]
    ArtifactChanged,
    #[error("artifact storage operation failed")]
    Io(#[from] io::Error),
}

/// Owns the on-disk artifact tree and every safe read/write beneath it.
///
/// Layout: `<root>/<run_id>/<relative_path>`. Files are copied from the fixed worktree
/// directory into a temporary file, size-checked, and atomically renamed before the manifest
/// row is written. The run id and every relative path are validated before joining, so a
/// corrupted manifest row can never escape the storage root.
#[derive(Clone, Debug)]
pub struct ScheduledArtifactStore {
    root: PathBuf,
    /// Serializes collection against the sweep: the sweep removes files with no manifest row,
    /// so it must never observe a run between its file rename and its manifest commit.
    lock: Arc<Mutex<()>>,
}

impl ScheduledArtifactStore {
    /// Builds a store rooted at the application data directory's artifact folder.
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            lock: Arc::new(Mutex::new(())),
        }
    }

    /// Returns the storage root so callers can create it during startup.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Acquires the process-wide collection/sweep lock.
    pub fn lock(&self) -> MutexGuard<'_, ()> {
        self.lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Collects whitelisted files from one run's worktree artifact directory.
    ///
    /// Missing directories are a normal "no artifacts" outcome. A symlinked or non-directory
    /// artifact directory is rejected as unsafe instead of followed. Individual candidates that
    /// fail path, type, link, count, or size checks are recorded in the rejected list and never
    /// copied.
    pub fn collect(
        &self,
        run_id: &ScheduledRunId,
        worktree_path: &Path,
        registered_at: i64,
    ) -> Result<CollectedScheduledArtifacts, ScheduledArtifactStoreError> {
        let run_directory = self.run_directory(run_id)?;
        let source_directory = worktree_path.join(SCHEDULED_ARTIFACT_DIRECTORY);
        let metadata = match fs::symlink_metadata(&source_directory) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(CollectedScheduledArtifacts::default());
            }
            Err(error) => return Err(error.into()),
        };
        if metadata.file_type().is_symlink() {
            return Ok(CollectedScheduledArtifacts {
                artifacts: Vec::new(),
                rejections: vec![ScheduledArtifactRejection {
                    name: SCHEDULED_ARTIFACT_DIRECTORY.to_string(),
                    reason: ScheduledArtifactRejectionReason::Symlink,
                }],
            });
        }
        if !metadata.is_dir() {
            return Ok(CollectedScheduledArtifacts {
                artifacts: Vec::new(),
                rejections: vec![ScheduledArtifactRejection {
                    name: SCHEDULED_ARTIFACT_DIRECTORY.to_string(),
                    reason: ScheduledArtifactRejectionReason::PathUnsafe,
                }],
            });
        }

        collection::collect_into(&source_directory, run_directory, registered_at)
    }

    /// Removes files that no longer have a manifest row and reports rows whose file vanished
    /// or changed size.
    ///
    /// The caller deletes the returned relative paths from the manifest, then deletes the run
    /// directory when both files and rows are gone. Symlinks are removed rather than followed.
    pub fn reconcile_run(
        &self,
        run_id: &ScheduledRunId,
        registered: &[ScheduledRunArtifact],
    ) -> Result<Vec<String>, ScheduledArtifactStoreError> {
        let run_directory = self.run_directory(run_id)?;
        let mut expected = HashMap::new();
        let mut mismatched = Vec::new();
        for artifact in registered {
            let Some(relative) = parse_artifact_relative_path(&artifact.relative_path) else {
                mismatched.push(artifact.relative_path.clone());
                continue;
            };
            let path = relative.to_path(&run_directory);
            match fs::symlink_metadata(&path) {
                Ok(metadata)
                    if metadata.is_file()
                        && !metadata.file_type().is_symlink()
                        && metadata.len() == artifact.size_bytes =>
                {
                    expected.insert(artifact.relative_path.clone(), ());
                }
                _ => mismatched.push(artifact.relative_path.clone()),
            }
        }

        if self.storage_directory_present(&run_directory)? {
            remove_unregistered_entries(&run_directory, "", &expected)?;
        }
        Ok(mismatched)
    }

    /// Returns whether `path` is a real directory chain with no link or reparse point in it.
    ///
    /// Cleanup must use this instead of `Path::exists`, because `exists` follows a junction and
    /// would let deletion escape the artifact root.
    fn storage_directory_present(&self, path: &Path) -> Result<bool, ScheduledArtifactStoreError> {
        match check_directory_without_symlinks(path) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error.into()),
        }
    }

    /// Deletes one run's whole storage directory, ignoring an already-missing directory.
    pub fn delete_run(&self, run_id: &ScheduledRunId) -> Result<(), ScheduledArtifactStoreError> {
        let run_directory = self.run_directory(run_id)?;
        if !self.storage_directory_present(&run_directory)? {
            return Ok(());
        }
        match fs::remove_dir_all(run_directory) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    /// Lists stored run directories so the sweep can remove directories with no manifest.
    pub fn list_run_directories(
        &self,
    ) -> Result<Vec<(String, PathBuf)>, ScheduledArtifactStoreError> {
        if !self.storage_directory_present(&self.root)? {
            return Ok(Vec::new());
        }
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let mut directories = Vec::new();
        for entry in entries {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if !file_type.is_dir() || file_type.is_symlink() {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            if parse_run_id(&name).is_some() {
                directories.push((name, entry.path()));
            }
        }
        Ok(directories)
    }

    /// Removes run directories that no longer own a manifest row.
    ///
    /// A directory can outlive its rows when collection crashed after copying files but before
    /// the manifest commit, or when a delete removed the rows but failed to unlink the tree.
    pub fn remove_run_directories_not_in(
        &self,
        known: &HashSet<String>,
    ) -> Result<(), ScheduledArtifactStoreError> {
        for (name, path) in self.list_run_directories()? {
            if known.contains(&name) {
                continue;
            }
            if !self.storage_directory_present(&path)? {
                continue;
            }
            match fs::remove_dir_all(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }

    /// Resolves one registered artifact to a verified regular file inside the run directory.
    pub fn artifact_path(
        &self,
        run_id: &ScheduledRunId,
        artifact: &ScheduledRunArtifact,
    ) -> Result<PathBuf, ScheduledArtifactStoreError> {
        let relative = parse_artifact_relative_path(&artifact.relative_path)
            .ok_or(ScheduledArtifactStoreError::UnsafeArtifactPath)?;
        let path = relative.to_path(&self.run_directory(run_id)?);
        if let Some(parent) = path.parent() {
            // Refuse a storage tree whose ancestors were replaced by links pointing outside;
            // the final file is checked separately because it may itself be a symlink.
            check_directory_without_symlinks(parent)?;
        }
        let metadata = fs::symlink_metadata(&path)?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() != artifact.size_bytes
        {
            return Err(ScheduledArtifactStoreError::ArtifactChanged);
        }
        Ok(path)
    }

    /// Reads one registered artifact after verifying its path and unchanged size.
    pub fn read_artifact(
        &self,
        run_id: &ScheduledRunId,
        artifact: &ScheduledRunArtifact,
    ) -> Result<Vec<u8>, ScheduledArtifactStoreError> {
        Ok(fs::read(self.artifact_path(run_id, artifact)?)?)
    }

    /// Returns the validated per-run directory, or an error when the run id is unsafe.
    fn run_directory(
        &self,
        run_id: &ScheduledRunId,
    ) -> Result<PathBuf, ScheduledArtifactStoreError> {
        let relative =
            parse_run_id(run_id.as_ref()).ok_or(ScheduledArtifactStoreError::InvalidRunId)?;
        Ok(relative.to_path(&self.root))
    }
}

/// Removes every file under a run directory that the manifest does not expect.
fn remove_unregistered_entries(
    directory: &Path,
    prefix: &str,
    expected: &HashMap<String, ()>,
) -> Result<(), ScheduledArtifactStoreError> {
    let mut entries = fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        let relative = if prefix.is_empty() {
            name.into_owned()
        } else {
            format!("{prefix}/{name}")
        };
        let file_type = entry.file_type()?;
        if file_type.is_symlink() || file_type.is_file() {
            if !expected.contains_key(&relative) {
                fs::remove_file(entry.path())?;
            }
            continue;
        }
        if file_type.is_dir() {
            remove_unregistered_entries(&entry.path(), &relative, expected)?;
            // Removing an empty directory keeps the tree free of stale nested folders.
            let _ = fs::remove_dir(entry.path());
        }
    }
    Ok(())
}

/// Parses one storage-relative path, rejecting Windows alternate data stream syntax.
fn parse_artifact_relative_path(raw: &str) -> Option<StrictRelativePath> {
    if raw.contains(':') {
        return None;
    }
    StrictRelativePath::parse(raw).ok()
}

/// Parses a run id as one safe storage segment.
fn parse_run_id(raw: &str) -> Option<StrictRelativePath> {
    if raw.contains(':') || raw.contains('/') || raw.contains('\\') {
        return None;
    }
    StrictRelativePath::parse(raw).ok()
}

#[cfg(test)]
mod tests;
