# Guardian

English | [中文](guardian.zh.md)

`ora-process-guardian` (`apps/ora-process-guardian`) is an independent process that owns one Scope's
Runs. The [host](host.md) launches it once per Scope; afterwards it survives the host and the Node, and
the next host instance rebinds to it instead of relaunching. The thin app delegates bootstrap, takeover
and Run operations to `ora-process-runtime`.

## Bootstrap

The host commits the Scope's creation intent and a consumed launch record before exec. A dedicated
socketpair delivers the bootstrap identity, and the original exclusive Scope flock is inherited without
unlocking and relocking. The child enters its own session, clears its environment and closes unintended
descriptors at exec; only `--bootstrap` appears in argv, and no parent-death or `Drop`-kill policy is
installed.

The guardian validates the inherited open description, its Scope inode, private paths and the local
filesystem. Only it initializes `guardian.sqlite` (WAL, `synchronous=FULL`, `user_version = 4`), then
publishes `control.sock`, `events.sock` and `io.sock`. `Ready` follows initialization; it proves neither
that a Run started nor that cleanup completed. Launch errors, lost replies and guardian death never
authorize another guardian for the same Scope, and existing journals, locks and endpoints are never
repaired automatically.

## Takeover

`GuardianManagement::bind` receives the host binding committed under `HostState`. A higher binding
commits before acknowledgement, same-binding retries are idempotent, and lower epochs or a different
instance at the same epoch are rejected. A host session carries the public binding only, not a bearer
credential. Requests are checked against current journal authority under the execution mutex, including
requests queued before a takeover, and an unfinished transaction or storage failure never produces a
successful takeover acknowledgement.

Messages use bounded MessagePack (16 KiB, depth 16), guardian wire version 3, with a five-second I/O
deadline per exchange and independently bounded channel workers. The guardian reconciles lifecycle state
every 50 ms independently of host connections.

Once Closed and every accepted Run's terminal cleanup facts have committed, periodic reconciliation
becomes quiescent instead of repeatedly reading and writing lifecycle journals. The guardian retains
its original lock, endpoints and bounded output, and continues checking takeover, query and output
requests. Unknown state and closure that has not committed continue to reconcile.

## Runs

After `Ready` and bind, the host uses `GuardianRuns::execute`:

- **Start** requires a never-reused RunId, an exact `RunSpec` and an explicit host-disconnect policy
  (`KeepRunning`). The guardian commits the spec and an uncertain initial snapshot before exec. The same
  id and spec return the original attempt; a changed spec conflicts; even a proven exec failure cannot be
  retried under that id. Environment values are stored verbatim in the private journal.
- **Query** returns the journaled snapshot, separating direct exit from best-effort cleanup. A lost
  response is not evidence of non-acceptance.
- **Stop** forces cleanup of one Run; **Close** durably seals the Scope before forcing cleanup. Both are
  safe to repeat, and closure does not prevent queries. Signal delivery is not a terminal result.
- **Output** reads an independent stdout or stderr prefix through `io.sock`. Capture is opt-in per
  `RunSpec`: at most 1 MiB per stream, 4096 bytes per read and 64 accepted attempts per Scope. Truncation
  and EOF are explicit, and bytes disappear with the guardian.

`RunSpec` liveness defaults to `Independent`. `TerminateOnOwnerExit` pins the observed owner process
before exec and force-cleans the Run when that pidfd exits, independent of host connectivity; the Node
uses it so its own death triggers cleanup. The owner identity is only a trigger, never signal authority.

Workloads use the [Linux best-effort adapter](linux-rootless.md) in separate sessions. Descendants that
escape before discovery may survive. If the guardian is killed, workloads may stay alive: old numeric
PIDs never become signal authority, stored `Running` facts become historical, and neither the guardian nor
the Run is respawned.

## Compatibility

Wire version 3 rejects older live peers. Live older guardians speak the earlier token protocol and need
their compatible management version: never rewrite their journals, replace their binary while they are
responsible, or restart old Scopes. Running workloads are not adopted across versions.
