//! A power-cut simulator for SQLite durability tests.
//!
//! Real power loss cannot be reproduced inside a test process: the operating
//! system's page cache survives process death, so killing connections only
//! proves crash safety, not power-loss safety. The missing piece is that the
//! machine itself forgets every write the application never asked it to fsync.
//! This module supplies exactly that at the VFS layer: every file operation is
//! delegated to the platform VFS unchanged, the simulator remembers what each
//! named file held at its last successful sync, and "pulling the plug" makes
//! every later open serve only that synced content. Handles that were live
//! when the plug was pulled fail their I/O like the handles of a dead process,
//! which also prevents their close-time checkpoints from resurrecting unsynced
//! data. Under `synchronous = NORMAL` the write-ahead log is never synced at
//! commit, so a cut rolls acknowledged transactions back; under
//! `synchronous = FULL` every commit syncs the WAL and survives the cut.
//!
//! After the cut, a reopened pool maps one wal-index from many connections, so
//! those locks live in the simulator instead of the platform. A successful delete
//! drops that path from the snapshot: SQLite removes the rollback journal when
//! it enables WAL without asking for a directory sync, and resurrecting the
//! journal makes recovery ignore the write-ahead log.

mod io;

use std::collections::HashMap;
use std::ffi::CString;
use std::os::raw::{c_int, c_void};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use rusqlite::ffi;

/// Distinguishes simulators so parallel tests each get their own machine.
static NEXT_SIMULATOR_ID: AtomicU32 = AtomicU32::new(1);

/// SQLite's wal-index has this many independent locks (`SQLITE_SHM_NLOCK`).
const SHM_LOCK_SLOTS: u32 = 8;

/// Everything the simulated machine remembers between power-on and power-cut.
struct MachineState {
    /// Flips once when the plug is pulled; never flips back.
    crashed: AtomicBool,
    /// Platform handles opened before the cut. Their close-time deletes must
    /// not erase the snapshot, so deletes are ignored until this hits zero.
    dying_handles: AtomicUsize,
    /// File content as of its last successful sync, by path. This is exactly
    /// what a power cut leaves behind on a real machine.
    durable: Mutex<HashMap<String, Vec<u8>>>,
    /// The world after the cut: content shared per path so every connection of
    /// a reopened pool observes one consistent machine.
    after_crash: Mutex<AfterCrash>,
    /// In-process wal-index locks for post-crash files. The platform locks do
    /// not cover memory files, and the pool maps one index from many connections.
    shm_locks: Mutex<HashMap<u64, ShmHolder>>,
    /// Hands each post-crash file a lock identity released from `xClose`.
    next_shm_holder: AtomicU64,
}

/// Memory-backed state that only exists once the machine has come back up.
struct AfterCrash {
    files: HashMap<String, Arc<Mutex<Vec<u8>>>>,
    /// Wal-index regions per path. Pointers into the boxed slices stay stable
    /// even while the outer vectors grow, which the shared-memory contract of
    /// `xShmMap` requires.
    shm: HashMap<String, Vec<Box<[u64]>>>,
}

/// Shared and exclusive wal-index bits held by one post-crash file handle.
struct ShmHolder {
    path: String,
    shared: u8,
    exclusive: u8,
}

/// Whether a platform file was opened by the process that is about to die.
#[derive(Clone, Copy)]
enum PlatformFileLifetime {
    /// Closing this handle retires one dying-process file, which is what
    /// re-enables deletes once that process has finished exiting.
    DyingProcess,
    /// Temporary file of a process that started after the cut.
    Restarted,
}

impl MachineState {
    /// Reports whether the plug has already been pulled.
    fn is_crashed(&self) -> bool {
        self.crashed.load(Ordering::SeqCst)
    }

    /// Pulls the plug: unsynced writes are gone and live handles are dead.
    fn crash(&self) {
        self.crashed.store(true, Ordering::SeqCst);
    }

    /// Counts a platform handle that still belongs to the dying process.
    fn note_dying_open(&self) {
        self.dying_handles.fetch_add(1, Ordering::SeqCst);
    }

    /// Retires one dying-process handle. Deletes stay suppressed until the last
    /// one closes, so `drop` of the dead pool cannot checkpoint the snapshot away.
    fn note_dying_close(&self) {
        let previous = self.dying_handles.fetch_sub(1, Ordering::SeqCst);
        assert!(
            previous > 0,
            "dying platform handle closed without a matching open"
        );
    }

    /// Reports whether the dead process still has a file open.
    fn dying_process_still_open(&self) -> bool {
        self.dying_handles.load(Ordering::SeqCst) > 0
    }

    /// Allocates the lock identity `xClose` uses to drop every slot this file held.
    fn allocate_shm_holder(&self) -> u64 {
        self.next_shm_holder.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Reads what the platform file currently holds.
    ///
    /// An absent file is empty, the same way a freshly created file reads back.
    /// Any other I/O error must fail the test: treating it as an empty snapshot
    /// would record a power cut that never happened.
    fn current_content(path: &str) -> Vec<u8> {
        match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => {
                panic!("reading {path} for the durability snapshot failed: {error}");
            }
        }
    }

    /// Records the file's current content as the durability baseline, keeping
    /// any baseline a previous sync of this machine already established.
    ///
    /// A later open must not replace that baseline with a dirty page-cache read.
    /// Durable deletes remove the entry first, so a recreated file can seed again.
    fn seed_durable(&self, path: &str) {
        let mut durable = self.durable.lock().expect("durable map lock");
        durable
            .entry(path.to_owned())
            .or_insert_with(|| Self::current_content(path));
    }

    /// Refreshes the durability baseline after a successful sync: everything
    /// the file holds right now has reached stable storage.
    fn mark_durable(&self, path: &str) {
        self.durable
            .lock()
            .expect("durable map lock")
            .insert(path.to_owned(), Self::current_content(path));
    }

    /// Drops a file whose deletion SQLite has made durable.
    fn forget_durable(&self, path: &str) {
        self.durable.lock().expect("durable map lock").remove(path);
    }

    /// Reports whether a restarted open should see this path.
    ///
    /// Existence is the durable snapshot plus files the restarted process has
    /// already created. The live page cache is not consulted: it still holds
    /// writes the cut was supposed to forget.
    fn visible_after_crash(&self, path: &str) -> bool {
        if self
            .after_crash
            .lock()
            .expect("post-crash lock")
            .files
            .contains_key(path)
        {
            return true;
        }
        self.durable
            .lock()
            .expect("durable map lock")
            .contains_key(path)
    }

    /// Returns the post-crash content for one path, seeding it from what the
    /// power cut preserved the first time it is touched.
    fn memory_file(&self, path: &str) -> Arc<Mutex<Vec<u8>>> {
        let mut after_crash = self.after_crash.lock().expect("post-crash lock");
        if let Some(existing) = after_crash.files.get(path) {
            return existing.clone();
        }
        let preserved = self
            .durable
            .lock()
            .expect("durable map lock")
            .get(path)
            .cloned()
            .unwrap_or_default();
        let memory = Arc::new(Mutex::new(preserved));
        after_crash.files.insert(path.to_owned(), memory.clone());
        memory
    }

    /// Applies a delete from the restarted process.
    ///
    /// The bytes are removed from both the snapshot and the restarted image so
    /// a later open cannot replay a WAL checkpoint already deleted. Mapped
    /// wal-index pages are zeroed in place; freeing them would dangle pointers
    /// SQLite still holds.
    fn delete_restarted_file(&self, path: &str) {
        {
            let mut after_crash = self.after_crash.lock().expect("post-crash lock");
            after_crash.files.remove(path);
            if let Some(regions) = after_crash.shm.get_mut(path) {
                for region in regions {
                    region.fill(0);
                }
            }
        }
        self.forget_durable(path);
    }

    /// Resolves one wal-index region, following the platform contract: a
    /// missing region with `extend` unset maps to a null pointer and success,
    /// which is what makes SQLite fall back to rebuilding the wal-index from
    /// the recovered WAL instead of failing the open.
    fn shm_region(
        &self,
        path: &str,
        index: usize,
        size: usize,
        extend: bool,
        out: *mut *mut c_void,
    ) {
        if out.is_null() {
            return;
        }
        let mut after_crash = self.after_crash.lock().expect("post-crash lock");
        let regions = after_crash.shm.entry(path.to_owned()).or_default();
        if index < regions.len() {
            unsafe { *out = regions[index].as_ptr().cast::<c_void>().cast_mut() };
            return;
        }
        if extend && regions.len() == index && size > 0 {
            regions.push(vec![0u64; size.div_ceil(8)].into_boxed_slice());
            unsafe { *out = regions[index].as_ptr().cast::<c_void>().cast_mut() };
            return;
        }
        unsafe { *out = std::ptr::null_mut() };
    }

    /// Acquires or releases wal-index locks for one post-crash file.
    ///
    /// The same handle may upgrade a shared slot to exclusive when no other
    /// handle holds it. Contention returns `SQLITE_BUSY` immediately: blocking
    /// inside `xShmLock` can deadlock SQLite, which backs off through the
    /// connection busy handler instead.
    fn shm_lock(
        &self,
        holder_id: u64,
        path: &str,
        offset: c_int,
        count: c_int,
        flags: c_int,
    ) -> c_int {
        let mask = shm_range_mask(offset, count);
        let mut table = self.shm_locks.lock().expect("shm lock table");
        match try_shm_lock(&mut table, holder_id, path, mask, flags) {
            ShmLockResult::Acquired => ffi::SQLITE_OK,
            ShmLockResult::Busy => ffi::SQLITE_BUSY,
        }
    }

    /// Drops every wal-index slot one file still holds.
    fn release_shm_holder(&self, holder_id: u64) {
        self.shm_locks
            .lock()
            .expect("shm lock table")
            .remove(&holder_id);
    }
}

/// Outcome of one non-blocking attempt to change a holder's wal-index bits.
enum ShmLockResult {
    Acquired,
    Busy,
}

/// Applies one lock or unlock if it does not conflict with other holders.
fn try_shm_lock(
    table: &mut HashMap<u64, ShmHolder>,
    holder_id: u64,
    path: &str,
    mask: u8,
    flags: c_int,
) -> ShmLockResult {
    table.entry(holder_id).or_insert_with(|| ShmHolder {
        path: path.to_owned(),
        shared: 0,
        exclusive: 0,
    });

    if (flags & ffi::SQLITE_SHM_UNLOCK) != 0 {
        let holder = table.get_mut(&holder_id).expect("shm holder");
        if (flags & ffi::SQLITE_SHM_SHARED) != 0 {
            holder.shared &= !mask;
        }
        if (flags & ffi::SQLITE_SHM_EXCLUSIVE) != 0 {
            holder.exclusive &= !mask;
        }
        return ShmLockResult::Acquired;
    }

    let (others_shared, others_exclusive) = other_shm_bits(table, holder_id, path);
    let holder = table.get_mut(&holder_id).expect("shm holder");
    if (flags & ffi::SQLITE_SHM_EXCLUSIVE) != 0 {
        if (others_shared | others_exclusive) & mask != 0 {
            return ShmLockResult::Busy;
        }
        // Upgrading drops this handle's shared bits so readers do not observe
        // both modes on a slot this connection now owns exclusively.
        holder.exclusive |= mask;
        holder.shared &= !mask;
        return ShmLockResult::Acquired;
    }
    if (flags & ffi::SQLITE_SHM_SHARED) != 0 && (others_exclusive & mask) != 0 {
        return ShmLockResult::Busy;
    }
    if (flags & ffi::SQLITE_SHM_SHARED) != 0 {
        holder.shared |= mask;
    }
    ShmLockResult::Acquired
}

/// ORs the shared and exclusive bits every other handle holds on `path`.
fn other_shm_bits(table: &HashMap<u64, ShmHolder>, holder_id: u64, path: &str) -> (u8, u8) {
    table
        .iter()
        .filter(|(id, holder)| **id != holder_id && holder.path == path)
        .fold((0u8, 0u8), |(shared, exclusive), (_, holder)| {
            (shared | holder.shared, exclusive | holder.exclusive)
        })
}

/// Builds the bit mask SQLite names with `offset` and `n` into the 8 lock slots.
fn shm_range_mask(offset: c_int, count: c_int) -> u8 {
    if offset < 0 || count <= 0 {
        return 0;
    }
    let offset = offset as u32;
    if offset >= SHM_LOCK_SLOTS {
        return 0;
    }
    let last = (offset + count as u32).min(SHM_LOCK_SLOTS);
    let mut mask = 0u8;
    for bit in offset..last {
        mask |= 1 << bit;
    }
    mask
}

/// The registered VFS plus the handle a test uses to drive its machine.
#[repr(C)]
struct CrashVfs {
    /// Must stay first: SQLite treats the leading bytes as `sqlite3_vfs`.
    base: ffi::sqlite3_vfs,
    /// The platform VFS every live operation is delegated to.
    underlying: *mut ffi::sqlite3_vfs,
    /// Registered name, referenced by the `vfs=` URI parameter.
    name: CString,
    state: Arc<MachineState>,
    /// Method table shared by every file this VFS opens.
    io: ffi::sqlite3_io_methods,
}

// The VFS is registered once and then only read; all mutation happens through
// the mutex-protected `MachineState`.
unsafe impl Sync for CrashVfs {}

/// One opened file. SQLite allocates the memory and calls `xClose` exactly
/// once per successful `xOpen`, so the Rust-owned fields are initialized in
/// `xOpen` and dropped in `xClose`.
#[repr(C)]
struct CrashFile {
    /// Must stay first: SQLite treats every `sqlite3_file` as this header.
    base: ffi::sqlite3_file,
    state: Arc<MachineState>,
    /// Empty for SQLite's unnamed temporary files, which hold no durable state.
    path: String,
    /// The platform file this handle wraps; null for post-crash memory files.
    underlying: *mut ffi::sqlite3_file,
    /// Backing allocation of `underlying`; emptied once it is closed.
    underlying_storage: Vec<u64>,
    /// Shared post-crash content; absent while the machine is powered on.
    memory: Option<Arc<Mutex<Vec<u8>>>>,
    /// Set for platform files opened before the cut.
    lifetime: PlatformFileLifetime,
    /// Zero until this post-crash file takes a wal-index lock identity.
    shm_holder: u64,
}

impl CrashFile {
    /// Reports whether this handle still wraps a platform file.
    fn wraps_platform_file(&self) -> bool {
        !self.underlying.is_null()
    }
}

/// Test-facing handle for one simulated machine.
pub(super) struct Simulator {
    vfs_name: String,
    state: Arc<MachineState>,
}

impl Simulator {
    /// Pulls the plug on this machine.
    pub(super) fn crash(&self) {
        self.state.crash();
    }

    /// Builds the SQLite URI that routes `DatabaseLocation::path` and plain
    /// `Connection::open` through this simulator's VFS.
    pub(super) fn database_uri(&self, directory: &Path) -> String {
        let database = directory.join("ora.sqlite3");
        format!(
            "file:{}?vfs={}",
            encode_sqlite_uri_path(&database),
            self.vfs_name
        )
    }
}

/// Percent-encodes a filesystem path for a SQLite `file:` URI.
///
/// Reserved characters would otherwise be parsed as the query or fragment, and
/// the `vfs` parameter that selects this simulator would be dropped. The drive
/// colon and slashes stay literal so Windows paths still parse.
fn encode_sqlite_uri_path(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b':' | b'.' | b'-' | b'_' | b'~' => {
                encoded.push(char::from(byte));
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

/// Registers a fresh power-cut simulator with a process-unique VFS name.
pub(super) fn register_simulator() -> Simulator {
    let id = NEXT_SIMULATOR_ID.fetch_add(1, Ordering::Relaxed);
    let name = CString::new(format!("ora-power-cut-{id}")).expect("VFS name is valid");
    let state = Arc::new(MachineState {
        crashed: AtomicBool::new(false),
        dying_handles: AtomicUsize::new(0),
        durable: Mutex::new(HashMap::new()),
        after_crash: Mutex::new(AfterCrash {
            files: HashMap::new(),
            shm: HashMap::new(),
        }),
        shm_locks: Mutex::new(HashMap::new()),
        next_shm_holder: AtomicU64::new(0),
    });

    // Safety: querying the default VFS only reads SQLite's global registry,
    // which `sqlite3_vfs_find` initializes on demand.
    let underlying = unsafe { ffi::sqlite3_vfs_find(std::ptr::null()) };
    assert!(!underlying.is_null(), "SQLite must expose a default VFS");

    let io = ffi::sqlite3_io_methods {
        iVersion: 2,
        xClose: Some(io::xclose),
        xRead: Some(io::xread),
        xWrite: Some(io::xwrite),
        xTruncate: Some(io::xtruncate),
        xSync: Some(io::xsync),
        xFileSize: Some(io::xfile_size),
        xLock: Some(io::xlock),
        xUnlock: Some(io::xunlock),
        xCheckReservedLock: Some(io::xcheck_reserved_lock),
        xFileControl: Some(io::xfile_control),
        xSectorSize: Some(io::xsector_size),
        xDeviceCharacteristics: Some(io::xdevice_characteristics),
        xShmMap: Some(io::xshm_map),
        xShmLock: Some(io::xshm_lock),
        xShmBarrier: Some(io::xshm_barrier),
        xShmUnmap: Some(io::xshm_unmap),
        xFetch: None,
        xUnfetch: None,
    };

    // Safety: reading the platform maximum pathname only inspects the
    // registered VFS that was just resolved above.
    let mx_pathname = unsafe { (*underlying).mxPathname };
    let base = ffi::sqlite3_vfs {
        iVersion: 2,
        szOsFile: std::mem::size_of::<CrashFile>() as c_int,
        mxPathname: mx_pathname,
        pNext: std::ptr::null_mut(),
        zName: name.as_ptr(),
        pAppData: std::ptr::null_mut(),
        xOpen: Some(io::xopen),
        xDelete: Some(io::xdelete),
        xAccess: Some(io::xaccess),
        xFullPathname: Some(io::xfull_pathname),
        xDlOpen: Some(io::xdl_open),
        xDlError: Some(io::xdl_error),
        xDlSym: Some(io::xdl_sym),
        xDlClose: Some(io::xdl_close),
        xRandomness: Some(io::xrandomness),
        xSleep: Some(io::xsleep),
        xCurrentTime: Some(io::xcurrent_time),
        xGetLastError: Some(io::xget_last_error),
        xCurrentTimeInt64: Some(io::xcurrent_time_int64),
        xSetSystemCall: None,
        xGetSystemCall: None,
        xNextSystemCall: None,
    };

    let mut simulator = Box::new(CrashVfs {
        base,
        underlying,
        name,
        state: state.clone(),
        io,
    });
    // Safety: registration only links the fully initialized VFS into
    // SQLite's registry; the struct stays alive for the process lifetime.
    let result = unsafe {
        ffi::sqlite3_vfs_register(&mut simulator.base, /*make_dflt*/ 0)
    };
    assert_eq!(
        result,
        ffi::SQLITE_OK,
        "registering the simulator VFS failed"
    );
    // The registration keeps the raw pointer, so the VFS must outlive it.
    let leaked = Box::leak(simulator);

    Simulator {
        vfs_name: leaked.name.to_string_lossy().into_owned(),
        state,
    }
}
