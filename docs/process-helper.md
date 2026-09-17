# Linux process helper deployment and management

English | [中文](process-helper.zh.md)

The independent privileged helper direction is approved in the
[Linux follow-up ADR](../specs/decisions/node/process/containment/linux/20260917-independent-privileged-helper.md).
The executable provides deployment preflight and an **authenticated inspection-only listener**.
It has no launch API, service installer or guardian integration, and does not advertise strong containment.

## Build and configuration

Build with `cargo build -p ora-process-helper`. A deployment administrator can explicitly run the
binary as a separate root process using `ora-process-helper --check /etc/ora/process-helper.json`.
Do not install it setuid. This change does not install binaries, create accounts, modify cgroups
or start privileged services.

Configuration version 1 uses this shape (the numeric identities are examples, not defaults):

```json
{
  "version": 1,
  "manager_uid": 1000,
  "workload_uid": 2000,
  "workload_gid": 2000,
  "cgroup_root": "/sys/fs/cgroup/ora-workloads"
}
```

The manager and workload UIDs must be different and nonzero; the workload GID must be nonzero.
Unknown fields and versions are rejected. Configuration is limited to 16 KiB and must be a regular
root-controlled file. Every ancestor must also be root-controlled, not group/other writable, and
free of symlinks. The shared `ora-utils::path::open_trusted_path` pins checked directory/file
descriptors instead of validating a path and following a replacement link afterward.

The cgroup root must already exist, be root-controlled, use cgroup v2, have `domain` type, contain
no directly attached processes and expose `cgroup.events` plus a writable `cgroup.kill`.
Preflight never writes to these files. The helper itself must remain outside the workload root.
Checking succeeds only for these prerequisites: it does not prove all descendants are gone,
freeze future permissions, verify account provisioning or grant permission to launch.

## Inspection service

An administrator can explicitly run `ora-process-helper --serve /etc/ora/process-helper.json /run/ora-helper/control.sock`.
The parent directory must already exist and satisfy the same root-controlled, non-writable-by-others,
no-symlink checks. The endpoint is owned by `manager_uid`, mode `0600`; its parent must allow that
manager to traverse it. Existing files or sockets are never replaced. SIGINT/SIGTERM closes the
listener and cancels exchanges, then removes only the socket inode created by this service.
Crash leftovers require administrator inspection/removal before restart; automatic recovery is pending.

Each connection is authenticated using the kernel's connecting peer UID before any payload is read.
The manager must not pass authenticated sockets to workloads: this is connection-time identity,
not per-message reauthentication. The manager can query availability, never choose a command, UID,
PID or cgroup target. Wire declarations belong to `ora-process-protocol`.

Each connection carries one request and one reply: a four-byte big-endian length followed by UTF-8 JSON.
Request: `{"version":1,"operation":"inspect"}`. Reply: `{"version":1,"status":"launch_unavailable"}`.
Other statuses are `unauthorized`, `invalid_request` and `unsupported_version`. Unknown fields are
rejected. Requests are limited to 16 KiB before allocation; the entire accepted exchange has a
five-second deadline, including reply writes. Truncated frames and timeouts close the connection.
At most 16 exchanges run concurrently; remaining connections stay in the bounded OS listen queue.
These transport limits are not workload shutdown policy. Inspection does not revalidate cgroup state.

## Remaining boundary

Execution before/after privilege dropping, no-new-privileges,
creation-time membership, descriptor handoff, workspace access under the workload identity,
guardian survival and helper recovery are still pending. The root helper must never become a
general-purpose arbitrary-root-command or arbitrary-PID migration interface.

Current tests exercise configuration rejection, non-cgroup filesystem rejection, CLI failure and
trusted path handling without privilege elevation. Real Unix-socket tests additionally cover peer
authentication, strict framing, timeout and listener shutdown on an unprivileged Linux runner.
Positive root/cgroup deployment and endpoint ownership/cleanup tests still require an
explicitly provisioned environment. The crates CI job now runs on Linux, macOS and Windows;
that matrix alone is not containment evidence.
