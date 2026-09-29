//! Creates owned directory trees without following links into another tree.
use std::{
    io,
    path::{Component, Path},
};

/// Creates each absolute-path component separately and refuses symlinks or non-directories.
/// Callers must own the tree: this checks existing layout, not concurrent hostile renames.
pub fn create_directories_without_symlinks(path: &Path) -> io::Result<()> {
    directories(path, MissingDirectory::Create)
}

/// Checks a directory and its ancestors without creating anything or following links.
pub fn check_directory_without_symlinks(path: &Path) -> io::Result<()> {
    directories(path, MissingDirectory::Reject)
}

enum MissingDirectory {
    Create,
    Reject,
}

/// Shares component checks between readers and creators.
fn directories(path: &Path, missing: MissingDirectory) -> io::Result<()> {
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "absolute directory required",
        ));
    }
    let mut current = std::path::PathBuf::new();
    for component in path.components() {
        if matches!(component, Component::ParentDir | Component::CurDir) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "non-normal directory",
            ));
        }
        current.push(component.as_os_str());
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "directory is a link or non-directory",
                ));
            }
            Err(error)
                if error.kind() == io::ErrorKind::NotFound
                    && matches!(missing, MissingDirectory::Create) =>
            {
                std::fs::create_dir(&current)?
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}
