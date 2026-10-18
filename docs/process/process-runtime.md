# Process Runtime

English | [中文](process-runtime.zh.md)

The process runtime runs a Node's managed programs (today, Git) so that their launch, exit and cleanup
are durable facts rather than in-memory assumptions. It is three cooperating pieces, all Linux-only and
trusted-local:

```text
ora-node ──IPC──▶ ora-process-host ──IPC──▶ ora-process-guardian (one per Scope) ──▶ workload Runs
           host.sock / host-io.sock         control.sock / events.sock / io.sock
```

- The **host** (`apps/ora-process-host`) owns durable Scope and Run intent and coordinates guardians,
  see [Host](host.md).
- Each **guardian** (`apps/ora-process-guardian`) is an independent process owning one Scope's Runs and
  their observations, and outlives the host and the Node, see [Guardian](guardian.md).
- The **lifecycle kernel** (`ora-process-runtime::ScopeRuntime`) and a platform adapter decide what
  each observation proves; on Linux the adapter is [rootless best effort](linux-rootless.md).

Desktop does not use this stack: its child processes go through `ora-process` and the
[reaper](../desktop/desktop-host.md#process-cleanup). The design decisions are the
[process ADRs](https://github.com/ora-space/specs/blob/main/decisions/node/process/README.md), with core
test cases under [`test-cases/node/process/`](https://github.com/ora-space/specs/blob/main/test-cases/node/process/README.md).

## Crates

| Crate | Owns |
|---|---|
| `ora-process-protocol` | Run identity, exact launch specification (`RunSpec`), containment selection, stop intent, exit facts and cleanup evidence; bounded MessagePack host and guardian messages; the helper's inspection wire types |
| `ora-process-client` | Clients for the host and guardian, depending on protocol types only |
| `ora-process-runtime` | `ScopeRuntime`, the Linux adapter, `HostState`, `HostCoordinator` and guardian internals |

## Lifecycle kernel

`ScopeRuntime<P>` owns one Scope's admission, Run records and stop deadlines. The `Platform` supplies
verified capabilities, creation-time containment, observations and per-Run signals.

- Scope creation freezes the actual guarantee. Required strong containment is rejected when
  unavailable, and requested best effort is never silently promoted.
- Replaying a RunId with identical parameters returns its current facts; changed parameters conflict.
  Unknown launches are never retried.
- Closing seals admission before scheduling cleanup; stopping one Run leaves its Scope and peers open.
  Wait, notify-then-wait and force requests only tighten deadlines or escalate.
- Direct exit and descendant cleanup are separate facts. A cleanup policy notifies descendants and
  forces them after its grace; wait-for-all keeps them managed until they exit or are stopped.
- Signal delivery alone never proves cleanup, and failed observations keep responsibility. Exit status
  may become more precise but never less; contradictory observations stay blocked.

Calls are serialized through mutable ownership, and the owner drives `reconcile` with monotonic
`Instant`s: the kernel has no background task, timer or `Drop`-based cleanup, and dropping it is not
crash recovery. Platform methods must be bounded and keep stable attempt identities, including after an
uncertain spawn.

## What is and is not guaranteed

- Containment is **best effort** on Linux: descendants that escape before discovery may survive, and
  completion is `BestEffortComplete`, never confirmed quiescence. The privileged helper route that would
  provide strong cgroup containment exposes only deployment checks and inspection, see
  [Linux helper](linux-helper.md); the cgroup, macOS and Windows containment decisions are `approved` but
  not implemented here.
- The deployment trusts the local Node and management programs: there are no secret tokens, signing keys
  or Controller authorization on these channels. Private paths and kernel peer-UID checks prevent
  accidental cross-user access, not malicious same-UID code. Host and guardian epochs keep stale
  cooperating instances from acting after a takeover.
- Output is captured as bounded, volatile prefixes; stdin is closed. Plugin processes are not yet run
  inside these Scopes, see [Node agent sessions](../node/agent-sessions.md#record-order-and-crashes).
- SQLite journals (host, guardian) enable WAL with `synchronous=FULL` and require a linked SQLite of at
  least 3.51.3, which contains the WAL-reset fix; physical power-loss behavior is not verified.

## File locks

`ora_utils::fs::LinuxFileLock` is the flock primitive under the guardian bootstrap. It acquires an
already opened regular file exclusively without waiting (`WouldBlock` on contention); cloning duplicates
the locked description and `into_file()` hands it to a child without unlocking and relocking.
Descriptors default to close-on-exec. `Drop` only closes the descriptor and never issues an explicit
unlock, which could release a child's shared lock, following Linux flock lifetime semantics. Callers
must preserve the inode, resolve trusted paths and qualify the filesystem; the lock proves neither
durability nor workload cleanup.
