# Process runtime implementation status

English | [中文](process-runtime.zh.md)

The [approved process ADRs](../specs/decisions/node/process/README.md) are being implemented incrementally.
The current increment provides an in-memory lifecycle kernel, **not a production process launcher**.
Existing `ora-process`, `ora-reaper`, Git and plugin entry points are unchanged.

Linux also has an independent [helper deployment preflight and authenticated inspection service](process-helper.md). Its checks
do not enable workload launch or constitute a platform adapter.

## Ownership and behavior

- `ora-process-protocol` owns local domain types: run identity, exact launch specification, containment
  selection, stop intent, direct exit facts and cleanup evidence. It does not yet define wire encoding.
- `ora-process-runtime::ScopeRuntime<P>` owns one scope's admission, run records and stop deadlines.
  `Platform` supplies verified capabilities, creation-time containment, observations and per-run signals.
  There is no OS adapter in this increment; tests inject platform facts through this boundary.
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

Durable acceptance, authorization, leases, host/guardian processes, platform adapters, I/O, recovery,
resource handoff and production integration remain unimplemented. No filesystem layout is changed.
This increment does not complete implementation phase 1 or prove any OS-level containment guarantee.

## Verification

Run `cargo test -p ora-process-runtime` and
`cargo clippy -p ora-process-protocol -p ora-process-runtime --all-targets -- -D warnings`.
Integration tests exercise the public runtime interface with controlled platform facts and time,
without sleeps or environment mutation. Evidence is recorded as `Partial` in the
[core case index](../specs/test-cases/node/process/README.md); real descendants, crashes, persistence,
cross-process races and platform permission boundaries still require direct verification.
