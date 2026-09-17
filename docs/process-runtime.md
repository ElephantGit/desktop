# Process runtime implementation status

English | [中文](process-runtime.zh.md)

The [approved process ADRs](../specs/decisions/node/process/README.md) are being implemented incrementally.
The current increment provides an in-memory lifecycle kernel and a
[rootless Linux best-effort adapter](process-linux-rootless.md), **not a production process launcher**.
Existing `ora-process`, `ora-reaper`, Git and plugin entry points are unchanged.

Linux also has an independent [helper deployment preflight and authenticated inspection service](process-helper.md). Its checks
do not enable workload launch or constitute a platform adapter. A low-level pre-exec launch gate now
exists for trusted helper code; it is not exposed over IPC and awaits privileged acceptance testing.

## Ownership and behavior

- `ora-process-protocol` owns local domain types: run identity, exact launch specification, containment
  selection, stop intent, direct exit facts and cleanup evidence, plus the helper's inspection-only
  wire types. Host/guardian wire encoding is not yet defined.
- `ora-process-runtime::ScopeRuntime<P>` owns one scope's admission, run records and stop deadlines.
  `Platform` supplies verified capabilities, creation-time containment, observations and per-run signals.
  Linux has a rootless adapter; controlled tests also inject platform facts through this boundary.
- Scope creation freezes the actual guarantee. Required strong containment is rejected when unavailable;
  explicitly requested best-effort containment is never silently promoted to strong containment.
- Replaying a RunId with identical parameters returns its current facts; changed parameters conflict.
  Unknown launches are never retried. Proven non-starts are currently replayed too, not resumed.
- Closing seals admission before scheduling cleanup. Stopping one run leaves its scope and peers open.
  Wait, notify-then-wait and force requests only tighten existing deadlines or escalate actions.
- Direct exit and descendant cleanup are separate. The cleanup policy notifies descendants and forces
  them after its explicit grace period; wait-for-all leaves them managed until they exit or are stopped.
- Signal delivery alone never proves cleanup. Failed observations and signals retain responsibility.
  Direct running/exit evidence confirms an uncertain launch without another spawn. Exit status may
  become more precise but never less precise; contradictory launch/exit observations remain blocked.

## Caller obligations and remaining work

Calls are serialized through mutable ownership. The caller must drive `reconcile` with monotonic
`Instant` values; this kernel has no background task, timer, retry backoff or Drop-based cleanup.
Platform methods must be bounded and must retain stable attempt identities, including after uncertain
spawn outcomes. Dropping the kernel does not provide crash recovery.

Linux now supports [bounded volatile result capture](process-linux-rootless.md#bounded-result-capture),
with independent readers and per-run limits; pipe EOF is separate from process cleanup.
Durable acceptance, authorization, leases, host/guardian processes, remaining platform adapters, full I/O, recovery,
resource handoff and production integration remain unimplemented. No filesystem layout is changed.
This increment does not complete implementation phase 1 or prove any OS-level containment guarantee.

## Guardian bootstrap foundation

The approved [rootless guardian bootstrap decision](../specs/decisions/node/process/recovery/20260917-rootless-guardian-bootstrap-and-reconnect.md)
now has its first building block: `ora_utils::fs::LinuxFileLock`. It accepts an already opened
regular file, attempts exclusive acquisition without waiting, and returns `WouldBlock` for contention.
Cloning duplicates the locked open file description; `into_file()` transfers it for explicit child
handoff without releasing/reacquiring the lock. Descriptors default to close-on-exec.
An unrelated concurrent fork can still hold a temporary copy before exec; closing the local owner
does not promise immediate reacquisition. Callers must observe actual lock acquisition.

Drop only closes a descriptor: it deliberately does not issue an explicit unlock, which could also
release a child's shared lock. This follows Linux's [flock lifetime semantics](https://man7.org/linux/man-pages/man2/flock.2.html).
The caller must preserve the inode, resolve trusted paths and qualify the actual local filesystem.
This primitive neither creates nor deletes files, authenticates inherited descriptors, proves data
durability, nor proves workload cleanup. It may acquire an unlocked file; it is not a bootstrap
verification API. Same-user adversaries, network filesystems and other platforms are not certified.

`cargo test -p ora-utils --test linux_file_lock` verifies contention, duplicate lifetime, unchanged
file contents, close-on-exec (including an explicit pre-exec barrier), and an exec'd holder retaining exclusion after its launcher is killed
externally. The surviving holder is identified and killed through a pidfd; acquisition must become
possible again. The child test fixture is not a guardian or host implementation. State-directory
admission, persistent creation intent, descriptor authentication, bootstrap, the guardian app and
Ready/reconnect operations remain to be implemented; no new remote launch endpoint is exposed.

## Verification

Run `cargo test -p ora-process-runtime` and
`cargo clippy -p ora-process-protocol -p ora-process-runtime --all-targets -- -D warnings`.
Controlled integration tests exercise the public runtime interface with injected platform facts and
time. Rootless Linux tests additionally exercise real children and descendants with readiness
handshakes and bounded polling. Neither mutates the test runner's environment. Evidence remains
`Partial` in the [core case index](../specs/test-cases/node/process/README.md); crashes, persistence,
exhaustive identity races and strong platform permission boundaries still require direct verification.
