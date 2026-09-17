use std::fs::File;
use std::io;
use std::os::fd::AsRawFd;

/// An exclusive Linux flock tied to an open file description, not a process or pathname.
///
/// The caller owns trusted path resolution, stable inode retention and filesystem qualification.
/// Never unlink/replace the lock file or explicitly unlock a duplicated descriptor. This is an
/// advisory coordination primitive, not authorization or proof that other processes have exited.
/// Concurrent fork children can temporarily retain a reference until exec closes it; local Drop
/// therefore does not promise immediate reacquisition by another open description.
#[derive(Debug)]
pub struct LinuxFileLock {
    file: File,
}

impl LinuxFileLock {
    /// Attempts acquisition without waiting or changing file contents; contention is WouldBlock.
    ///
    /// Supply a freshly opened regular file, or an exclusively locked inherited description.
    /// This can acquire an unlocked file: it does not authenticate an alleged inherited lock.
    pub fn try_acquire(file: File) -> io::Result<Self> {
        if !file.metadata()?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "lock requires a regular file",
            ));
        }
        // Keep accidental exec inheritance disabled. Intended handoff must explicitly map a
        // duplicate into the target child; changing this flag does not affect sibling descriptors.
        // SAFETY: fcntl only queries/updates flags on this owned live descriptor.
        let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFD) };
        if flags < 0
            || unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0
        {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: flock borrows the live file and LOCK_NB prevents waiting for another owner.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { file })
    }

    /// Duplicates the same locked description rather than releasing and reopening its path.
    pub fn try_clone(&self) -> io::Result<Self> {
        Ok(Self {
            file: self.file.try_clone()?,
        })
    }

    /// Transfers the still-locked descriptor for explicit child handoff; no unlock occurs here.
    ///
    /// The returned file stays close-on-exec until the caller deliberately maps it into a child.
    /// It must only be passed to trusted code, which must not call LOCK_UN on any duplicate.
    pub fn into_file(self) -> File {
        self.file
    }
}

// Deliberately no unlocking Drop: LOCK_UN would release the shared lock even while a child owns
// another descriptor. File's close releases this reference; the last reference releases the lock.
