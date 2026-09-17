# Linux process helper deployment preflight

English | [中文](process-helper.zh.md)

The independent privileged helper direction is approved in the
[Linux follow-up ADR](../specs/decisions/node/process/containment/linux/20260917-independent-privileged-helper.md).
The current executable implements **deployment preflight only**. It has no launch API, listener,
service installer or guardian integration, and does not advertise strong containment.

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

## Remaining boundary

Authenticated management IPC, execution before/after privilege dropping, no-new-privileges,
creation-time membership, descriptor handoff, workspace access under the workload identity,
guardian survival and helper recovery are still pending. The root helper must never become a
general-purpose arbitrary-root-command or arbitrary-PID migration interface.

Current tests exercise configuration rejection, non-cgroup filesystem rejection, CLI failure and
trusted path handling without privilege elevation. Positive root/cgroup tests still require an
explicitly provisioned environment. The crates CI job now runs on Linux, macOS and Windows;
that matrix alone is not containment evidence.
