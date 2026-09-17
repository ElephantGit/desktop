# Rootless Linux process tracking

English | [中文](process-linux-rootless.zh.md)

`ora_process_runtime::LinuxBestEffort::with_discarded_io()` provides the first real Linux
adapter for `ScopeRuntime`. It needs no root, sudo, privileged helper, cgroup delegation,
service installation or separate workload account. Workloads retain the caller's identity.
The privileged [helper route](process-helper.md) is preserved but is not the current priority.

## Admission and ownership

The adapter advertises only `BestEffortOnly`. `RequireStrong` is rejected before launch;
`PreferStrong` selects `BestEffort` before accepting work. Completion is
`BestEffortComplete`, never `ConfirmedQuiescence`.

Construction checks readable procfs, pidfd operations and `waitid(P_PIDFD)` support, and
rejects automatic child reaping via `SIGCHLD` ignore or `SA_NOCLDWAIT`. Restricted kernels,
procfs mounts or syscall policies can reject construction; rootless does not mean every
container or gVisor configuration is supported. Procfs must describe the caller's PID namespace.

Every run starts a new session before exec. Its environment is exactly `RunSpec.env`, not an
implicit inheritance of the host environment. This constructor explicitly discards all three
standard streams; it is not yet suitable for Git/plugin integration that needs output or input.

The caller must drive `ScopeRuntime::reconcile` and exclusively own child reaping. No other
thread or signal handler may reap these children or enable automatic reaping while tracking.
The direct child remains unreaped until tracked cleanup completes, preserving the session ID's
identity even after direct exit. A post-launch pidfd acquisition failure keeps ownership and
reports an uncertain launch; later observations retry acquisition, never launch a duplicate.

## Discovery, stopping and evidence

- Scan the original session for members, pin their proc directories during pidfd acquisition,
  and retain captured pidfds when members later detach or are reparented. Do not infer ownership
  from historical PPIDs or broadcast signals to a numeric process group.
- Notify sends `SIGTERM`; force sends `SIGKILL`. Stop intent persists so later discoveries receive
  it too. Failed discovery does not prevent signaling already captured members.
- Signal through pidfds. Only the exclusively owned, unreaped direct child may use `Child::kill`
  as a force fallback if pidfd acquisition/signaling fails; that failure still remains visible.
- Report best-effort completion only when direct exit and all captured exits preceded a fresh,
  successful scan that found no new identity, and all captured members are still exited. Reap the
  direct child only then. Scan/signaling errors and live captured members prevent completion.
- Dropping the adapter attempts force cleanup and starts a direct-child reaper. This is not a
  cleanup certificate and does not survive a crash or `SIGKILL` of the owner.

Descendants that create another session **before discovery**, or new descendants born outside the
original session, may escape tracking. PID handles prevent redirecting signals to a recycled PID;
they do not make discovery exhaustive or prevent escape. This is intentionally best effort, not
a security boundary against same-user workloads.

## Verification and remaining work

Run `cargo test -p ora-process-runtime --test linux_best_effort` and
`cargo test -p ora-utils --test linux_process` as an ordinary Linux user. Real-process tests cover
admission, replay without duplicate execution, direct exit with surviving descendants, run isolation,
notify/force escalation, captured-member `setsid`, and Drop's direct-child cleanup. Utility tests cover
pidfd exit/reaping, stale proc observations and non-UTF-8 process names. They do not force actual
numeric PID reuse or exhaust descriptors to verify post-launch acquisition recovery.

Durable host/guardian ownership, crash recovery, I/O handoff and production entry points remain
unimplemented. See the [runtime status](process-runtime.md). No strong-containment ADR is completed
by these tests, and Linux results do not establish Windows/macOS support.
