use std::ffi::OsString;
use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};

/// An exclusive, process-wide OS lease on a `<name>.lock` sibling of a data file.
///
/// The lease is deliberately taken on a sidecar rather than on the data file itself. Storage
/// engines such as SQLite place their own locks on the file they open, and a whole-file lease on
/// that same inode collides with them outside Linux: macOS shares one kernel lock table between
/// `flock` and `fcntl`, so the engine's byte-range request fails with "database is locked", and
/// Windows `LockFileEx` regions are mandatory, so the engine's own reads fail with a lock
/// violation. A sibling inode still resolves identically for every spelling of the same
/// directory, so alternate paths cannot acquire a second lease.
///
/// This is advisory coordination between cooperating processes, not authorization; the caller
/// owns trusted path resolution. Contention is reported as [`io::ErrorKind::WouldBlock`].
#[derive(Debug)]
pub struct SidecarLease {
    file: File,
    path: PathBuf,
}

impl SidecarLease {
    /// Locks `<target>.lock` beside `target` without waiting, creating the sidecar when absent.
    ///
    /// The sidecar's contents are never read or written; only its lock state matters.
    pub fn try_acquire(target: &Path) -> io::Result<Self> {
        let path = Self::path_for(target)?;
        let file = OpenOptions::new()
            .read(/*read*/ true)
            .write(/*write*/ true)
            .create(/*create*/ true)
            .truncate(/*truncate*/ false)
            .open(&path)?;
        match file.try_lock() {
            Ok(()) => Ok(Self { file, path }),
            Err(TryLockError::WouldBlock) => Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                format!("another process holds {}", path.display()),
            )),
            Err(TryLockError::Error(error)) => Err(error),
        }
    }

    /// Returns the sidecar path guarding `target`, rejecting targets without a file name.
    pub fn path_for(target: &Path) -> io::Result<PathBuf> {
        let Some(name) = target.file_name() else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "lease target must name a file",
            ));
        };
        let mut lock_name = OsString::from(name);
        lock_name.push(".lock");
        Ok(target.with_file_name(lock_name))
    }

    /// The sidecar file this lease holds.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for SidecarLease {
    /// Unlocks explicitly rather than relying on close: a descriptor transiently duplicated into
    /// a concurrently spawned child would otherwise keep the lease alive after this owner is gone.
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn guards_a_sibling_lock_file_and_reports_contention_as_would_block() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("data.sqlite3");
        let lease = SidecarLease::try_acquire(&target).unwrap();
        assert_eq!(lease.path(), dir.path().join("data.sqlite3.lock"));
        assert!(lease.path().is_file());
        // The data file itself stays untouched so its owner can lock it however it likes.
        assert!(!target.exists());
        let contended = SidecarLease::try_acquire(&target).unwrap_err();
        assert_eq!(contended.kind(), io::ErrorKind::WouldBlock);
        // An alternate spelling of the same directory resolves to the same sidecar inode.
        let alias = dir.path().join("nested").join("..").join("data.sqlite3");
        std::fs::create_dir(dir.path().join("nested")).unwrap();
        assert_eq!(
            SidecarLease::try_acquire(&alias).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        drop(lease);
        drop(SidecarLease::try_acquire(&target).unwrap());
    }

    #[test]
    fn rejects_targets_without_a_file_name() {
        assert_eq!(
            SidecarLease::path_for(Path::new("/")).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }
}
