# Rootless guardian bootstrap

English | [中文](process-guardian.zh.md)

Linux now has a real independent `ora-process-guardian` app and an `ora-process-client` read-only
Ready probe. No root, helper or service installation is required. This is the first bootstrap slice
of the [approved decision](../specs/decisions/node/process/recovery/20260917-rootless-guardian-bootstrap-and-reconnect.md),
not durable Run execution or production Node integration.

## Ownership and ordering

The caller explicitly injects the private state path and trusted guardian executable into
[HostState](process-host-state.md). The executable cannot be set-id; neither paths nor credentials
are inferred from HOME. Creation intent and a one-shot launch record commit before exec.
The child enters its own session with an empty environment; no parent-death signal or Drop-kill
policy is installed. The host reserves a reaper thread before exec. Host termination does not
terminate the guardian, although an external service manager may still kill an entire service group.

Only `--bootstrap` is passed on the command line. A dedicated socketpair delivers the versioned
MessagePack bootstrap frame; stdin carries the original locked file description and stdout carries
that socket, not logs. Other descriptors are close-on-exec. The child restores CLOEXEC on inherited
bootstrap slots. Kernels without the required `close_range` support reject launch.

`LinuxFileLock::adopt_inherited` verifies the inherited description's exclusive flock through procfs;
an unlocked file, reopened inode or shared lock cannot acquire bootstrap qualification. The guardian
also validates private paths, filesystem, canonical Scope name and the lock's device/inode. It
refuses existing journals or unknown directory contents without removing them. Root and the same UID
remain trusted; this is not hostile same-UID isolation.

Only the guardian initializes `guardian.sqlite`, with its original identity, creating-host binding
and credential in a WAL/FULL transaction. It syncs the private output directory and Scope directory,
then publishes `control.sock`, `events.sock` and `io.sock`. A Ready response therefore follows
initialization and endpoint binding. Bootstrap EOF is not a liveness lease. Lock files, journals
and endpoint paths are preserved on exit; old Scope initialization never becomes available again.

## Readiness is not control takeover

All three sockets currently support only readiness probes: no Run, mutation, event subscription or
workload I/O message exists. Each exchange checks kernel peer UID, credential, protocol version,
original intent and actual socket role. The client correlates replies with one random session nonce.
Frames are length-prefixed MessagePack, bounded to 16 KiB and depth 16; malformed messages, unknown
fields and trailing objects fail. Each exchange has a five-second deadline. Independent worker
limits prevent stalled I/O handshakes from consuming the control channel's slots.

The nonce is read-only correlation, **not** a persisted management session or execution fence.
A replacement host can recover the original credential and discover that same live guardian;
it cannot acknowledge a new controlling epoch. Durable host takeover, execution-time fencing,
Run authorization/acceptance, lost-host policy and rootless workload I/O are the next slice.

Every failure after launch recording remains `launch_unknown`, including exec failure and lost
caller replies. Query the original instance; do not retry exec, replace locks, unlink stale sockets,
or reopen an old guardian journal to adopt running workloads. A dead guardian does not prove cleanup.
Partial initialization is preserved for later explicit recovery work, not automatically repaired.

## Verification

`cargo test -p ora-process-guardian --test bootstrap` uses the real app in a private temporary
deployment. It verifies journal-before-Ready, a separate session, wrong credentials/UID rejection,
three-channel correlation, stalled-I/O isolation, invalid inherited qualification, existing-journal
preservation, consumed failed-exec attempts, and original-instance discovery after external launcher
SIGKILL. The launcher fixture uses production APIs but is not a production host app.

Protocol tests cover bounded framing, canonical identities, unknown fields and secret redaction;
utils tests cover inherited OFD qualification and stripping unintended inheritable descriptors.
Host tests cover exact v1 migration without identity or lock replacement. Arbitrary crash-window
injection, physical power loss, service-manager deployment, Windows/macOS guardian support and
durable Run recovery remain unverified. The ADR remains approved, not implemented.
