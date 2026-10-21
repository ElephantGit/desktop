//! Safe traversal and copy of the fixed worktree artifact directory.

use std::collections::HashSet;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ora_domain::{
    MAX_SCHEDULED_ARTIFACT_BYTES, MAX_SCHEDULED_ARTIFACT_TOTAL_BYTES,
    MAX_SCHEDULED_ARTIFACTS_PER_RUN, ScheduledArtifactKind, ScheduledArtifactRejection,
    ScheduledArtifactRejectionReason, ScheduledRunArtifact,
};
use ora_utils::path::{
    StrictRelativePath, create_directories_without_symlinks, has_multiple_hard_links,
};

use super::{
    CollectedScheduledArtifacts, ScheduledArtifactStoreError, parse_artifact_relative_path,
};

static TEMPORARY_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Walks the already-validated artifact directory and copies every accepted candidate.
pub(super) fn collect_into(
    source_directory: &Path,
    run_directory: PathBuf,
    registered_at: i64,
) -> Result<CollectedScheduledArtifacts, ScheduledArtifactStoreError> {
    let mut state = CollectionState {
        run_directory,
        registered_at,
        names: HashSet::new(),
        total_bytes: 0,
        collected: CollectedScheduledArtifacts::default(),
    };
    visit_directory(source_directory, "", &mut state)?;
    Ok(state.collected)
}

/// Accumulates one collection pass while enforcing the frozen per-run bounds.
struct CollectionState {
    run_directory: PathBuf,
    registered_at: i64,
    names: HashSet<String>,
    total_bytes: u64,
    collected: CollectedScheduledArtifacts,
}

/// Walks the artifact directory without following symlinks and copies accepted files.
fn visit_directory(
    directory: &Path,
    prefix: &str,
    state: &mut CollectionState,
) -> Result<(), ScheduledArtifactStoreError> {
    let mut entries = fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            state.collected.rejections.push(ScheduledArtifactRejection {
                name: file_name.to_string_lossy().into_owned(),
                reason: ScheduledArtifactRejectionReason::PathUnsafe,
            });
            continue;
        };
        let relative = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        };
        let Some(parsed) = parse_artifact_relative_path(&relative) else {
            state.collected.rejections.push(ScheduledArtifactRejection {
                name: relative,
                reason: ScheduledArtifactRejectionReason::PathUnsafe,
            });
            continue;
        };
        if !state.names.insert(parsed.fold_case_key()) {
            state.collected.rejections.push(ScheduledArtifactRejection {
                name: relative,
                reason: ScheduledArtifactRejectionReason::PathUnsafe,
            });
            continue;
        }
        let metadata = match fs::symlink_metadata(entry.path()) {
            Ok(metadata) => metadata,
            Err(_) => {
                state.collected.rejections.push(ScheduledArtifactRejection {
                    name: relative,
                    reason: ScheduledArtifactRejectionReason::ReadFailed,
                });
                continue;
            }
        };
        if metadata.file_type().is_symlink() {
            state.collected.rejections.push(ScheduledArtifactRejection {
                name: relative,
                reason: ScheduledArtifactRejectionReason::Symlink,
            });
            continue;
        }
        if metadata.is_dir() {
            visit_directory(&entry.path(), &relative, state)?;
            continue;
        }
        if !metadata.is_file() {
            state.collected.rejections.push(ScheduledArtifactRejection {
                name: relative,
                reason: ScheduledArtifactRejectionReason::PathUnsafe,
            });
            continue;
        }
        collect_file(&entry.path(), &parsed, relative, metadata, state);
    }
    Ok(())
}

/// Applies type, link, count, and size rules before copying one candidate file.
fn collect_file(
    source: &Path,
    relative: &StrictRelativePath,
    relative_name: String,
    metadata: fs::Metadata,
    state: &mut CollectionState,
) {
    let Some(kind) =
        ScheduledArtifactKind::from_file_name(relative.file_name().unwrap_or_default())
    else {
        state.collected.rejections.push(ScheduledArtifactRejection {
            name: relative_name,
            reason: ScheduledArtifactRejectionReason::UnsupportedType,
        });
        return;
    };
    match has_multiple_hard_links(source) {
        Ok(false) => {}
        Ok(true) => {
            state.collected.rejections.push(ScheduledArtifactRejection {
                name: relative_name,
                reason: ScheduledArtifactRejectionReason::HardLink,
            });
            return;
        }
        Err(_) => {
            state.collected.rejections.push(ScheduledArtifactRejection {
                name: relative_name,
                reason: ScheduledArtifactRejectionReason::ReadFailed,
            });
            return;
        }
    }
    if let Err(reason) = check_bounds(
        state.collected.artifacts.len(),
        state.total_bytes,
        metadata.len(),
    ) {
        state.collected.rejections.push(ScheduledArtifactRejection {
            name: relative_name,
            reason,
        });
        return;
    }

    match copy_verified(source, relative, &state.run_directory, &metadata) {
        Ok(()) => {
            state.total_bytes = state.total_bytes.saturating_add(metadata.len());
            state.collected.artifacts.push(ScheduledRunArtifact {
                relative_path: relative.as_str().to_string(),
                kind,
                size_bytes: metadata.len(),
                registered_at: state.registered_at,
            });
        }
        Err(CopyFailure::TooLarge) => {
            state.collected.rejections.push(ScheduledArtifactRejection {
                name: relative_name,
                reason: ScheduledArtifactRejectionReason::TooLarge,
            });
        }
        Err(CopyFailure::Changed) => {
            state.collected.rejections.push(ScheduledArtifactRejection {
                name: relative_name,
                reason: ScheduledArtifactRejectionReason::ReadFailed,
            });
        }
        Err(CopyFailure::Failed) => {
            state.collected.rejections.push(ScheduledArtifactRejection {
                name: relative_name,
                reason: ScheduledArtifactRejectionReason::CopyFailed,
            });
        }
    }
}

/// Checks one candidate against the frozen per-file, per-run count, and per-run total bounds.
fn check_bounds(
    collected_count: usize,
    collected_bytes: u64,
    candidate_bytes: u64,
) -> Result<(), ScheduledArtifactRejectionReason> {
    if candidate_bytes > MAX_SCHEDULED_ARTIFACT_BYTES {
        return Err(ScheduledArtifactRejectionReason::TooLarge);
    }
    if collected_count >= MAX_SCHEDULED_ARTIFACTS_PER_RUN {
        return Err(ScheduledArtifactRejectionReason::TooMany);
    }
    if collected_bytes.saturating_add(candidate_bytes) > MAX_SCHEDULED_ARTIFACT_TOTAL_BYTES {
        return Err(ScheduledArtifactRejectionReason::TotalTooLarge);
    }
    Ok(())
}

/// Classifies a copy failure so the rejected list can carry the exact reason.
enum CopyFailure {
    TooLarge,
    Changed,
    Failed,
}

/// Copies through a temporary file, verifies the byte count, then atomically renames.
fn copy_verified(
    source: &Path,
    relative: &StrictRelativePath,
    run_directory: &Path,
    source_metadata: &fs::Metadata,
) -> Result<(), CopyFailure> {
    let destination = relative.to_path(run_directory);
    let parent = destination
        .parent()
        .ok_or(CopyFailure::Failed)?
        .to_path_buf();
    create_directories_without_symlinks(run_directory).map_err(|_| CopyFailure::Failed)?;
    create_directories_without_symlinks(&parent).map_err(|_| CopyFailure::Failed)?;
    let temporary = run_directory.join(format!(
        ".partial-{}-{}",
        std::process::id(),
        TEMPORARY_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let copied = match copy_bounded(source, &temporary) {
        Ok(copied) => copied,
        Err(_) => {
            let _ = fs::remove_file(&temporary);
            return Err(CopyFailure::Failed);
        }
    };
    if copied > MAX_SCHEDULED_ARTIFACT_BYTES {
        let _ = fs::remove_file(&temporary);
        return Err(CopyFailure::TooLarge);
    }
    if copied != source_metadata.len() {
        let _ = fs::remove_file(&temporary);
        return Err(CopyFailure::Changed);
    }
    if destination.exists() {
        let _ = fs::remove_file(&destination);
    }
    fs::rename(&temporary, &destination).map_err(|_| {
        let _ = fs::remove_file(&temporary);
        CopyFailure::Failed
    })
}

/// Copies at most one byte beyond the single-artifact bound into a newly created file.
///
/// Bounding the read keeps a file that grows between its metadata probe and the copy from
/// filling the artifact volume before the oversize rejection is recorded.
fn copy_bounded(source: &Path, temporary: &Path) -> io::Result<u64> {
    let mut input = fs::File::open(source)?;
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temporary)?;
    io::copy(
        &mut input.by_ref().take(MAX_SCHEDULED_ARTIFACT_BYTES + 1),
        &mut output,
    )
}

#[cfg(test)]
mod tests {
    use ora_domain::{
        MAX_SCHEDULED_ARTIFACT_BYTES, MAX_SCHEDULED_ARTIFACT_TOTAL_BYTES,
        MAX_SCHEDULED_ARTIFACTS_PER_RUN,
    };
    use pretty_assertions::assert_eq;

    use super::check_bounds;
    use ora_domain::ScheduledArtifactRejectionReason;

    #[test]
    fn accepts_the_exact_single_artifact_bound_and_rejects_one_byte_more() {
        assert_eq!(check_bounds(0, 0, MAX_SCHEDULED_ARTIFACT_BYTES), Ok(()));
        assert_eq!(
            check_bounds(0, 0, MAX_SCHEDULED_ARTIFACT_BYTES + 1),
            Err(ScheduledArtifactRejectionReason::TooLarge)
        );
    }

    #[test]
    fn accepts_the_sixty_fourth_artifact_and_rejects_the_sixty_fifth() {
        assert_eq!(
            check_bounds(MAX_SCHEDULED_ARTIFACTS_PER_RUN - 1, 0, 1),
            Ok(())
        );
        assert_eq!(
            check_bounds(MAX_SCHEDULED_ARTIFACTS_PER_RUN, 0, 1),
            Err(ScheduledArtifactRejectionReason::TooMany)
        );
    }

    #[test]
    fn accepts_the_exact_total_bound_and_rejects_one_byte_more() {
        assert_eq!(
            check_bounds(0, MAX_SCHEDULED_ARTIFACT_TOTAL_BYTES - 1, 1),
            Ok(())
        );
        assert_eq!(
            check_bounds(0, MAX_SCHEDULED_ARTIFACT_TOTAL_BYTES, 1),
            Err(ScheduledArtifactRejectionReason::TotalTooLarge)
        );
    }
}
