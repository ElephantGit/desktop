# Linux Rootless Adapter

English | [中文](linux-rootless.zh.md)

`ora_process_runtime::LinuxBestEffort` is the Linux platform adapter for `ScopeRuntime`. It needs no
root, sudo, privileged helper, cgroup delegation, service installation or separate account; workloads
keep the caller's identity (the Node can additionally drop workloads to `process.workload_uid`). The
privileged alternative is described in [Linux helper](linux-helper.md).

## Admission

The adapter advertises only `BestEffortOnly`: `RequireStrong` is rejected before launch, `PreferStrong`
selects best effort, and completion is `BestEffortComplete`, never `ConfirmedQuiescence`. Construction
checks readable procfs (describing the caller's PID namespace), pidfd operations and
`waitid(P_PIDFD)`, and rejects automatic child reaping (`SIGCHLD` ignored or `SA_NOCLDWAIT`). Restricted
kernels, procfs mounts or syscall policies can reject construction, so not every container or gVisor
configuration is supported.

Every Run starts a new session before exec, with exactly `RunSpec.env` as its environment and stdin
closed. `with_discarded_io()` discards all three streams and rejects capture requests;
`with_bounded_output()` accepts `OutputPolicy::Capture { stdout_limit, stderr_limit }`.

## Bounded output

Limits are per-stream byte counts and part of the exact `RunSpec` replay identity; zero accepts only
empty output and exactly the limit is not overflow. Each captured pipe has an independent reader that
keeps draining without consumers, retaining only the bounded prefix. Overflow or a reader failure
requests force cleanup at the next `reconcile`, even under `WaitForAll`.

`ScopeRuntime::read_output(run, stream, offset, max_bytes)` copies without consuming and reports the
retained length, sticky truncation and `Open`, `Eof` or `Failed`; offsets past the prefix are errors.
An EOF with truncation is incomplete and a read failure is not EOF. Direct exit, tracked cleanup and pipe
EOF are independent: a descendant can hold a pipe after the direct child exits. Bytes are volatile and
there is no stdin, persistence, backpressure or rotation.

## Discovery, stopping and evidence

- The adapter scans the original session for members, pins their proc directories while acquiring
  pidfds, and keeps captured pidfds when members detach or are reparented. Ownership is never inferred
  from historical PPIDs, and signals are never broadcast to a numeric process group.
- Notify sends `SIGTERM` and force sends `SIGKILL`, through pidfds; stop intent persists so members
  discovered later receive it too. Only the owned, unreaped direct child may fall back to `Child::kill`.
- Best-effort completion is reported only when the direct exit and every captured exit preceded a fresh,
  successful scan that found no new identity. The direct child is reaped only then, so its session id
  keeps its identity until cleanup completes. Scan or signal errors and live members prevent completion.
- The caller must drive `reconcile` and exclusively own child reaping; no other thread or handler may
  reap these children. A post-launch pidfd failure keeps ownership and reports an uncertain launch, and
  later observations retry acquisition rather than launching again.
- Dropping the adapter attempts force cleanup and starts a direct-child reaper; this is not a cleanup
  certificate and does not survive a crash or `SIGKILL` of the owner.

Descendants that create another session before discovery, or are born outside the original session, may
escape. PID handles prevent signalling a recycled PID; they do not make discovery exhaustive. This is
intentionally not a security boundary against same-user workloads.

## Testing

Run `cargo test -p ora-process-runtime --test linux_best_effort`, `--test linux_output` and
`cargo test -p ora-utils --test linux_process --test pipe_capture` as an ordinary user. They use real
children and descendants with readiness handshakes and never mutate the test runner's environment.
