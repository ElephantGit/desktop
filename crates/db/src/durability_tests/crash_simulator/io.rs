//! VFS and file methods for the power-cut simulator.
//!
//! Named files opened after the cut are served from the durable snapshot.
//! Platform handles that were already open fail their I/O, which is what stops
//! a dying process from checkpointing unsynced frames back into that snapshot.

use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_void};
use std::sync::Mutex;
use std::sync::atomic::Ordering;

use rusqlite::ffi;

use super::{CrashFile, CrashVfs, PlatformFileLifetime};

/// Borrows the file handle SQLite handed to an I/O method.
unsafe fn crash_file(file: *mut ffi::sqlite3_file) -> &'static mut CrashFile {
    unsafe { &mut *(file as *mut CrashFile) }
}

/// Reads the method table of the wrapped platform file.
unsafe fn underlying_methods(handle: &CrashFile) -> &'static ffi::sqlite3_io_methods {
    unsafe { &*(*handle.underlying).pMethods }
}

/// Decodes the file name SQLite handed to `xOpen`, using an empty string for
/// its unnamed temporary files.
unsafe fn decode_name(z_name: *const c_char) -> String {
    if z_name.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(z_name) }
            .to_string_lossy()
            .into_owned()
    }
}

/// Copies `amount` bytes at `offset` out of shared memory, zero-filling what
/// lies past the end and reporting the short read SQLite expects.
unsafe fn read_memory(
    memory: &Mutex<Vec<u8>>,
    buffer: *mut c_void,
    amount: c_int,
    offset: i64,
) -> c_int {
    unsafe {
        let content = memory.lock().expect("post-crash file lock");
        let start = usize::try_from(offset).unwrap_or(usize::MAX);
        let amount = usize::try_from(amount).unwrap_or(0);
        let destination = std::slice::from_raw_parts_mut(buffer.cast::<u8>(), amount);
        let available = content.len().saturating_sub(start).min(amount);
        if available > 0 {
            destination[..available].copy_from_slice(&content[start..start + available]);
        }
        if available < amount {
            destination[available..].fill(0);
            return ffi::SQLITE_IOERR_SHORT_READ;
        }
        ffi::SQLITE_OK
    }
}

/// Copies `amount` bytes at `offset` into shared memory, growing it as needed.
unsafe fn write_memory(
    memory: &Mutex<Vec<u8>>,
    buffer: *const c_void,
    amount: c_int,
    offset: i64,
) -> c_int {
    unsafe {
        let mut content = memory.lock().expect("post-crash file lock");
        let start = usize::try_from(offset).unwrap_or(usize::MAX);
        let amount = usize::try_from(amount).unwrap_or(0);
        let source = std::slice::from_raw_parts(buffer.cast::<u8>(), amount);
        if start > content.len() {
            content.resize(start, 0);
        }
        let end = start.saturating_add(amount);
        if end > content.len() {
            content.resize(end, 0);
        }
        content[start..end].copy_from_slice(source);
        ffi::SQLITE_OK
    }
}

/// Opens a platform file and wraps it.
///
/// `lifetime` records whether this handle belongs to the process that will die
/// in the cut. Only those handles suppress snapshot deletes while they close.
unsafe fn open_platform_file(
    simulator: &CrashVfs,
    z_name: *const c_char,
    file: *mut ffi::sqlite3_file,
    flags: c_int,
    out_flags: *mut c_int,
    lifetime: PlatformFileLifetime,
) -> c_int {
    unsafe {
        // The platform file struct needs its own allocation matching the
        // platform VFS's declared size; u64 chunks satisfy its alignment.
        let storage_words = ((*simulator.underlying).szOsFile.max(0) as usize).div_ceil(8);
        let mut storage = vec![0u64; storage_words];
        let underlying = storage.as_mut_ptr().cast::<ffi::sqlite3_file>();
        let result = match (*simulator.underlying).xOpen {
            Some(open) => open(simulator.underlying, z_name, underlying, flags, out_flags),
            None => return ffi::SQLITE_ERROR,
        };
        if result != ffi::SQLITE_OK {
            // SQLite does not call xClose for a failed open, so nothing was
            // wrapped and the storage is simply dropped here.
            return result;
        }
        let track_dying = matches!(lifetime, PlatformFileLifetime::DyingProcess);
        // The buffer SQLite handed over is uninitialized, so the handle is
        // written wholesale with `ptr::write`: a plain field assignment would
        // first drop the garbage bytes as an Arc/String/Vec.
        std::ptr::write(
            file as *mut CrashFile,
            CrashFile {
                base: ffi::sqlite3_file {
                    pMethods: &simulator.io,
                },
                state: simulator.state.clone(),
                path: decode_name(z_name),
                underlying,
                underlying_storage: storage,
                memory: None,
                lifetime,
                shm_holder: 0,
            },
        );
        if track_dying {
            simulator.state.note_dying_open();
        }
        ffi::SQLITE_OK
    }
}

pub(super) unsafe extern "C" fn xopen(
    vfs: *mut ffi::sqlite3_vfs,
    z_name: ffi::sqlite3_filename,
    file: *mut ffi::sqlite3_file,
    flags: c_int,
    out_flags: *mut c_int,
) -> c_int {
    unsafe {
        let simulator = &*(vfs as *const CrashVfs);
        let state = simulator.state.clone();

        // SQLite passes a null name for its own temporary files. They hold no
        // durable database state, so the platform serves them directly whether
        // the machine is on or has come back from a cut.
        if z_name.is_null() {
            let lifetime = if state.is_crashed() {
                PlatformFileLifetime::Restarted
            } else {
                PlatformFileLifetime::DyingProcess
            };
            return open_platform_file(simulator, z_name, file, flags, out_flags, lifetime);
        }
        let path = decode_name(z_name);

        if state.is_crashed() {
            // A missing file without CREATE must not materialize an empty
            // snapshot: SQLite uses that failure to decide the WAL is absent.
            let creating = (flags & ffi::SQLITE_OPEN_CREATE) != 0;
            if !state.visible_after_crash(&path) && !creating {
                return ffi::SQLITE_CANTOPEN;
            }
            let memory = state.memory_file(&path);
            std::ptr::write(
                file as *mut CrashFile,
                CrashFile {
                    base: ffi::sqlite3_file {
                        pMethods: &simulator.io,
                    },
                    state,
                    path,
                    underlying: std::ptr::null_mut(),
                    underlying_storage: Vec::new(),
                    memory: Some(memory),
                    lifetime: PlatformFileLifetime::Restarted,
                    shm_holder: simulator.state.allocate_shm_holder(),
                },
            );
            if !out_flags.is_null() {
                *out_flags = flags;
            }
            return ffi::SQLITE_OK;
        }

        // Powered on: open through the platform VFS and remember what the
        // file holds right now as the baseline a cut would fall back to.
        let result = open_platform_file(
            simulator,
            z_name,
            file,
            flags,
            out_flags,
            PlatformFileLifetime::DyingProcess,
        );
        if result == ffi::SQLITE_OK {
            state.seed_durable(&path);
        }
        result
    }
}

pub(super) unsafe extern "C" fn xdelete(
    vfs: *mut ffi::sqlite3_vfs,
    z_name: *const c_char,
    sync_dir: c_int,
) -> c_int {
    unsafe {
        let simulator = &*(vfs as *const CrashVfs);
        if z_name.is_null() {
            return ffi::SQLITE_ERROR;
        }
        let path = decode_name(z_name);
        if simulator.state.is_crashed() {
            // The dying process still has its files open here. Honoring its
            // close-time delete would erase the only snapshot the cut kept.
            if simulator.state.dying_process_still_open() {
                return ffi::SQLITE_OK;
            }
            simulator.state.delete_restarted_file(&path);
            return ffi::SQLITE_OK;
        }
        let result = match (*simulator.underlying).xDelete {
            Some(delete) => delete(simulator.underlying, z_name, sync_dir),
            None => ffi::SQLITE_OK,
        };
        // Drop the snapshot whenever SQLite's delete succeeds. Enabling WAL
        // deletes the rollback journal without a directory sync; keeping that
        // journal makes the restarted process roll it back and ignore the WAL.
        if result == ffi::SQLITE_OK {
            simulator.state.forget_durable(&path);
        }
        let _ = sync_dir;
        result
    }
}

pub(super) unsafe extern "C" fn xaccess(
    vfs: *mut ffi::sqlite3_vfs,
    z_name: *const c_char,
    flags: c_int,
    out: *mut c_int,
) -> c_int {
    unsafe {
        let simulator = &*(vfs as *const CrashVfs);
        if simulator.state.is_crashed() {
            if out.is_null() {
                return ffi::SQLITE_ERROR;
            }
            let exists =
                !z_name.is_null() && simulator.state.visible_after_crash(&decode_name(z_name));
            // Exists, read, and read-write all follow the snapshot. The live
            // disk still contains writes the cut was supposed to lose.
            let _ = flags;
            *out = c_int::from(exists);
            return ffi::SQLITE_OK;
        }
        match (*simulator.underlying).xAccess {
            Some(access) => access(simulator.underlying, z_name, flags, out),
            None => ffi::SQLITE_ERROR,
        }
    }
}

pub(super) unsafe extern "C" fn xfull_pathname(
    vfs: *mut ffi::sqlite3_vfs,
    z_name: *const c_char,
    n_out: c_int,
    z_out: *mut c_char,
) -> c_int {
    unsafe {
        let simulator = &*(vfs as *const CrashVfs);
        match (*simulator.underlying).xFullPathname {
            Some(full_pathname) => full_pathname(simulator.underlying, z_name, n_out, z_out),
            None => ffi::SQLITE_ERROR,
        }
    }
}

pub(super) unsafe extern "C" fn xdl_open(
    vfs: *mut ffi::sqlite3_vfs,
    path: *const c_char,
) -> *mut c_void {
    unsafe {
        let simulator = &*(vfs as *const CrashVfs);
        match (*simulator.underlying).xDlOpen {
            Some(dl_open) => dl_open(simulator.underlying, path),
            None => std::ptr::null_mut(),
        }
    }
}

pub(super) unsafe extern "C" fn xdl_error(
    vfs: *mut ffi::sqlite3_vfs,
    n_byte: c_int,
    message: *mut c_char,
) {
    unsafe {
        let simulator = &*(vfs as *const CrashVfs);
        if let Some(dl_error) = (*simulator.underlying).xDlError {
            dl_error(simulator.underlying, n_byte, message);
        }
    }
}

pub(super) unsafe extern "C" fn xdl_sym(
    vfs: *mut ffi::sqlite3_vfs,
    handle: *mut c_void,
    symbol: *const c_char,
) -> Option<unsafe extern "C" fn(*mut ffi::sqlite3_vfs, *mut c_void, *const c_char)> {
    unsafe {
        let simulator = &*(vfs as *const CrashVfs);
        (*simulator.underlying)
            .xDlSym
            .and_then(|dl_sym| dl_sym(simulator.underlying, handle, symbol))
    }
}

pub(super) unsafe extern "C" fn xdl_close(vfs: *mut ffi::sqlite3_vfs, handle: *mut c_void) {
    unsafe {
        let simulator = &*(vfs as *const CrashVfs);
        if let Some(dl_close) = (*simulator.underlying).xDlClose {
            dl_close(simulator.underlying, handle);
        }
    }
}

pub(super) unsafe extern "C" fn xrandomness(
    vfs: *mut ffi::sqlite3_vfs,
    n_byte: c_int,
    z_out: *mut c_char,
) -> c_int {
    unsafe {
        let simulator = &*(vfs as *const CrashVfs);
        match (*simulator.underlying).xRandomness {
            Some(randomness) => randomness(simulator.underlying, n_byte, z_out),
            None => ffi::SQLITE_ERROR,
        }
    }
}

pub(super) unsafe extern "C" fn xsleep(vfs: *mut ffi::sqlite3_vfs, microseconds: c_int) -> c_int {
    unsafe {
        let simulator = &*(vfs as *const CrashVfs);
        match (*simulator.underlying).xSleep {
            Some(sleep) => sleep(simulator.underlying, microseconds),
            None => ffi::SQLITE_ERROR,
        }
    }
}

pub(super) unsafe extern "C" fn xcurrent_time(vfs: *mut ffi::sqlite3_vfs, now: *mut f64) -> c_int {
    unsafe {
        let simulator = &*(vfs as *const CrashVfs);
        match (*simulator.underlying).xCurrentTime {
            Some(current_time) => current_time(simulator.underlying, now),
            None => ffi::SQLITE_ERROR,
        }
    }
}

pub(super) unsafe extern "C" fn xget_last_error(
    vfs: *mut ffi::sqlite3_vfs,
    n_byte: c_int,
    z_out: *mut c_char,
) -> c_int {
    unsafe {
        let simulator = &*(vfs as *const CrashVfs);
        match (*simulator.underlying).xGetLastError {
            Some(get_last_error) => get_last_error(simulator.underlying, n_byte, z_out),
            None => ffi::SQLITE_ERROR,
        }
    }
}

pub(super) unsafe extern "C" fn xcurrent_time_int64(
    vfs: *mut ffi::sqlite3_vfs,
    now: *mut i64,
) -> c_int {
    unsafe {
        let simulator = &*(vfs as *const CrashVfs);
        match (*simulator.underlying).xCurrentTimeInt64 {
            Some(current_time) => current_time(simulator.underlying, now),
            None => ffi::SQLITE_ERROR,
        }
    }
}

pub(super) unsafe extern "C" fn xclose(file: *mut ffi::sqlite3_file) -> c_int {
    unsafe {
        let handle = crash_file(file);
        if matches!(handle.lifetime, PlatformFileLifetime::DyingProcess) {
            handle.state.note_dying_close();
            handle.lifetime = PlatformFileLifetime::Restarted;
        }
        if handle.shm_holder != 0 {
            handle.state.release_shm_holder(handle.shm_holder);
            handle.shm_holder = 0;
        }
        let mut result = ffi::SQLITE_OK;
        if handle.wraps_platform_file() {
            // Releasing the platform handle is exactly what process death
            // does; a crashed handle simply has nothing left to flush.
            if let Some(close) = underlying_methods(handle).xClose {
                result = close(handle.underlying);
            }
            handle.underlying = std::ptr::null_mut();
        }
        // The handle lives in SQLite-owned memory, so its Rust fields are
        // dropped here rather than by any Rust scope.
        std::ptr::drop_in_place(file as *mut CrashFile);
        result
    }
}

pub(super) unsafe extern "C" fn xread(
    file: *mut ffi::sqlite3_file,
    buffer: *mut c_void,
    amount: c_int,
    offset: i64,
) -> c_int {
    unsafe {
        let handle = crash_file(file);
        if let Some(memory) = &handle.memory {
            return read_memory(memory, buffer, amount, offset);
        }
        if handle.path.is_empty() || !handle.state.is_crashed() {
            if let Some(read) = underlying_methods(handle).xRead {
                return read(handle.underlying, buffer, amount, offset);
            }
            return ffi::SQLITE_OK;
        }
        ffi::SQLITE_IOERR_READ
    }
}

pub(super) unsafe extern "C" fn xwrite(
    file: *mut ffi::sqlite3_file,
    buffer: *const c_void,
    amount: c_int,
    offset: i64,
) -> c_int {
    unsafe {
        let handle = crash_file(file);
        if let Some(memory) = &handle.memory {
            return write_memory(memory, buffer, amount, offset);
        }
        if handle.path.is_empty() || !handle.state.is_crashed() {
            if let Some(write) = underlying_methods(handle).xWrite {
                return write(handle.underlying, buffer, amount, offset);
            }
            return ffi::SQLITE_OK;
        }
        ffi::SQLITE_IOERR_WRITE
    }
}

pub(super) unsafe extern "C" fn xtruncate(file: *mut ffi::sqlite3_file, size: i64) -> c_int {
    unsafe {
        let handle = crash_file(file);
        if let Some(memory) = &handle.memory {
            let length = usize::try_from(size).unwrap_or(0);
            memory
                .lock()
                .expect("post-crash file lock")
                .resize(length, 0);
            return ffi::SQLITE_OK;
        }
        if handle.path.is_empty() || !handle.state.is_crashed() {
            if let Some(truncate) = underlying_methods(handle).xTruncate {
                return truncate(handle.underlying, size);
            }
            return ffi::SQLITE_OK;
        }
        ffi::SQLITE_IOERR_WRITE
    }
}

pub(super) unsafe extern "C" fn xsync(file: *mut ffi::sqlite3_file, flags: c_int) -> c_int {
    unsafe {
        let handle = crash_file(file);
        // Post-crash memory already is the machine's only storage; nothing
        // more can be lost, so syncing is a no-op.
        if handle.memory.is_some() {
            return ffi::SQLITE_OK;
        }
        if handle.path.is_empty() || !handle.state.is_crashed() {
            let result = match underlying_methods(handle).xSync {
                Some(sync) => sync(handle.underlying, flags),
                None => ffi::SQLITE_OK,
            };
            if result == ffi::SQLITE_OK && !handle.path.is_empty() {
                handle.state.mark_durable(&handle.path);
            }
            return result;
        }
        ffi::SQLITE_IOERR_FSYNC
    }
}

pub(super) unsafe extern "C" fn xfile_size(file: *mut ffi::sqlite3_file, size: *mut i64) -> c_int {
    unsafe {
        let handle = crash_file(file);
        if let Some(memory) = &handle.memory {
            *size = memory.lock().expect("post-crash file lock").len() as i64;
            return ffi::SQLITE_OK;
        }
        if handle.path.is_empty() || !handle.state.is_crashed() {
            if let Some(file_size) = underlying_methods(handle).xFileSize {
                return file_size(handle.underlying, size);
            }
            return ffi::SQLITE_OK;
        }
        ffi::SQLITE_IOERR_FSTAT
    }
}

pub(super) unsafe extern "C" fn xlock(file: *mut ffi::sqlite3_file, lock: c_int) -> c_int {
    unsafe {
        let handle = crash_file(file);
        if handle.memory.is_some() {
            return ffi::SQLITE_OK;
        }
        if handle.path.is_empty() || !handle.state.is_crashed() {
            if let Some(lock_file) = underlying_methods(handle).xLock {
                return lock_file(handle.underlying, lock);
            }
            return ffi::SQLITE_OK;
        }
        ffi::SQLITE_IOERR_LOCK
    }
}

pub(super) unsafe extern "C" fn xunlock(file: *mut ffi::sqlite3_file, lock: c_int) -> c_int {
    unsafe {
        let handle = crash_file(file);
        if handle.memory.is_some() {
            return ffi::SQLITE_OK;
        }
        if handle.path.is_empty() || !handle.state.is_crashed() {
            if let Some(unlock) = underlying_methods(handle).xUnlock {
                return unlock(handle.underlying, lock);
            }
            return ffi::SQLITE_OK;
        }
        ffi::SQLITE_IOERR_UNLOCK
    }
}

pub(super) unsafe extern "C" fn xcheck_reserved_lock(
    file: *mut ffi::sqlite3_file,
    out: *mut c_int,
) -> c_int {
    unsafe {
        let handle = crash_file(file);
        if handle.memory.is_some() {
            *out = 0;
            return ffi::SQLITE_OK;
        }
        if handle.path.is_empty() || !handle.state.is_crashed() {
            if let Some(check) = underlying_methods(handle).xCheckReservedLock {
                return check(handle.underlying, out);
            }
            return ffi::SQLITE_OK;
        }
        ffi::SQLITE_IOERR_LOCK
    }
}

pub(super) unsafe extern "C" fn xfile_control(
    file: *mut ffi::sqlite3_file,
    op: c_int,
    arg: *mut c_void,
) -> c_int {
    unsafe {
        let handle = crash_file(file);
        if handle.memory.is_some() {
            // The simulator implements no file controls of its own.
            return ffi::SQLITE_NOTFOUND;
        }
        if handle.path.is_empty() || !handle.state.is_crashed() {
            if let Some(file_control) = underlying_methods(handle).xFileControl {
                return file_control(handle.underlying, op, arg);
            }
            return ffi::SQLITE_NOTFOUND;
        }
        ffi::SQLITE_NOTFOUND
    }
}

pub(super) unsafe extern "C" fn xsector_size(file: *mut ffi::sqlite3_file) -> c_int {
    unsafe {
        let handle = crash_file(file);
        if handle.memory.is_some() {
            return 512;
        }
        match underlying_methods(handle).xSectorSize {
            Some(sector_size) => sector_size(handle.underlying),
            None => 512,
        }
    }
}

pub(super) unsafe extern "C" fn xdevice_characteristics(file: *mut ffi::sqlite3_file) -> c_int {
    unsafe {
        let handle = crash_file(file);
        if handle.memory.is_some() {
            return 0;
        }
        match underlying_methods(handle).xDeviceCharacteristics {
            Some(characteristics) => characteristics(handle.underlying),
            None => 0,
        }
    }
}

pub(super) unsafe extern "C" fn xshm_map(
    file: *mut ffi::sqlite3_file,
    region_index: c_int,
    region_size: c_int,
    extend: c_int,
    out: *mut *mut c_void,
) -> c_int {
    unsafe {
        let handle = crash_file(file);
        if handle.memory.is_some() {
            // A missing region without the extend flag maps to a null pointer
            // with success, the same way the platform VFSes behave; that is
            // what makes SQLite rebuild the wal-index from the recovered WAL.
            handle.state.shm_region(
                &handle.path,
                region_index.max(0) as usize,
                region_size.max(0) as usize,
                extend != 0,
                out,
            );
            return ffi::SQLITE_OK;
        }
        if handle.path.is_empty() || !handle.state.is_crashed() {
            if let Some(shm_map) = underlying_methods(handle).xShmMap {
                return shm_map(handle.underlying, region_index, region_size, extend, out);
            }
            return ffi::SQLITE_OK;
        }
        ffi::SQLITE_IOERR
    }
}

pub(super) unsafe extern "C" fn xshm_lock(
    file: *mut ffi::sqlite3_file,
    offset: c_int,
    n: c_int,
    flags: c_int,
) -> c_int {
    unsafe {
        let handle = crash_file(file);
        if handle.memory.is_some() {
            return handle
                .state
                .shm_lock(handle.shm_holder, &handle.path, offset, n, flags);
        }
        if handle.path.is_empty() || !handle.state.is_crashed() {
            if let Some(shm_lock) = underlying_methods(handle).xShmLock {
                return shm_lock(handle.underlying, offset, n, flags);
            }
            return ffi::SQLITE_OK;
        }
        ffi::SQLITE_IOERR_LOCK
    }
}

pub(super) unsafe extern "C" fn xshm_barrier(file: *mut ffi::sqlite3_file) {
    unsafe {
        let handle = crash_file(file);
        if handle.memory.is_some() {
            // Wal-index writes are ordinary stores. The barrier is what makes
            // them visible to the other pooled connections before the lock drops.
            std::sync::atomic::fence(Ordering::SeqCst);
            return;
        }
        if handle.state.is_crashed() {
            return;
        }
        if let Some(shm_barrier) = underlying_methods(handle).xShmBarrier {
            shm_barrier(handle.underlying);
        }
    }
}

pub(super) unsafe extern "C" fn xshm_unmap(
    file: *mut ffi::sqlite3_file,
    delete_flag: c_int,
) -> c_int {
    unsafe {
        let handle = crash_file(file);
        if handle.memory.is_some() {
            // Regions stay in simulator memory; they model files the cut kept.
            return ffi::SQLITE_OK;
        }
        if handle.path.is_empty() || !handle.state.is_crashed() {
            if let Some(shm_unmap) = underlying_methods(handle).xShmUnmap {
                return shm_unmap(handle.underlying, delete_flag);
            }
            return ffi::SQLITE_OK;
        }
        ffi::SQLITE_OK
    }
}
