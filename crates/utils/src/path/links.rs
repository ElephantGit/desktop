//! Detects whether a path has more than one hard link without following symlinks.

use std::io;
use std::path::Path;

/// Reports whether the file or directory at `path` has more than one hard link.
///
/// A multi-link file shares its inode with names that may live outside the tree a caller is
/// about to copy from, which is why artifact collection refuses such candidates.
#[cfg(unix)]
pub fn has_multiple_hard_links(path: &Path) -> io::Result<bool> {
    use std::os::unix::fs::MetadataExt;
    Ok(std::fs::symlink_metadata(path)?.nlink() > 1)
}

/// Queries the Win32 handle link count without following a reparse point.
#[cfg(windows)]
pub fn has_multiple_hard_links(path: &Path) -> io::Result<bool> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, GetFileInformationByHandle, OPEN_EXISTING,
    };

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: `wide` is a NUL-terminated path and the handle is closed before returning.
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let mut information: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: the handle is valid and `information` points to writable storage.
    let queried = unsafe { GetFileInformationByHandle(handle, &mut information) };
    // SAFETY: the handle was created above and is not used afterwards.
    unsafe {
        CloseHandle(handle);
    }
    if queried == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(information.nNumberOfLinks > 1)
}

/// Reports no detectable hard links on platforms without a portable link-count API.
#[cfg(not(any(unix, windows)))]
pub fn has_multiple_hard_links(_path: &Path) -> io::Result<bool> {
    Ok(false)
}
