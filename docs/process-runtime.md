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

## Verification

Run `cargo test -p ora-process-runtime` and
`cargo clippy -p ora-process-protocol -p ora-process-runtime --all-targets -- -D warnings`.
Controlled integration tests exercise the public runtime interface with injected platform facts and
time. Rootless Linux tests additionally exercise real children and descendants with readiness
handshakes and bounded polling. Neither mutates the test runner's environment. Evidence remains
`Partial` in the [core case index](../specs/test-cases/node/process/README.md); crashes, persistence,
exhaustive identity races and strong platform permission boundaries still require direct verification.
