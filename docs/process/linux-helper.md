# Linux Helper

English | [中文](linux-helper.zh.md)

`ora-process-helper` (`apps/ora-process-helper`) is the independent privileged helper that the
[Linux helper ADR](https://github.com/ora-space/specs/blob/main/decisions/node/process/containment/linux/20260917-independent-privileged-helper.md)
(`approved`) describes for strong cgroup v2 containment. What exists today is deployment preflight, an
authenticated **inspection-only** listener and a low-level launch gate that is not exposed over IPC.
There is no launch API, service installer or guardian integration, and nothing advertises strong
containment. Node deployments use the [rootless adapter](linux-rootless.md) instead and need no
privileged deployment.

## Configuration and preflight

Build with `cargo build -p ora-process-helper`. An administrator explicitly runs it as a separate root
process with `ora-process-helper --check /etc/ora/process-helper.json`; never install it setuid. Neither
`--check` nor `--serve` changes cgroup membership, and nothing installs binaries, creates accounts or
starts privileged services automatically.

```json
{
  "version": 1,
  "manager_uid": 1000,
  "workload_uid": 2000,
  "workload_gid": 2000,
  "cgroup_root": "/sys/fs/cgroup/ora-workloads"
}
```

The manager and workload UIDs differ and are non-zero, the GID is non-zero, and `4294967295` is rejected
for all three. Unknown fields and versions are rejected. The file is at most 16 KiB, a regular
root-controlled file whose ancestors are root-controlled, not group- or other-writable and free of
symlinks; `ora-utils::path::open_trusted_path` pins checked descriptors instead of re-following paths.

The cgroup root must already exist, be root-controlled, use cgroup v2 with `domain` type, contain no
directly attached processes and expose `cgroup.events` and a writable `cgroup.kill`; the helper itself
must stay outside it. Preflight never writes these files and succeeds only for these prerequisites; it
proves neither that descendants are gone nor that launching is allowed.

## Inspection service

`ora-process-helper --serve /etc/ora/process-helper.json /run/ora-helper/control.sock` listens on an
endpoint owned by `manager_uid` with mode `0600` in an existing root-controlled parent. Existing files or
sockets are never replaced; `SIGINT`/`SIGTERM` remove only the socket this service created, and crash
leftovers need administrator removal before restart.

Each connection is authenticated by the kernel's peer UID before any payload is read (connection-time
identity, so the manager must not pass sockets to workloads) and carries one request and one reply: a
four-byte big-endian length and UTF-8 JSON. The request is `{"version":1,"operation":"inspect"}`; replies
are `launch_unavailable`, `unauthorized`, `invalid_request` or `unsupported_version`. Requests are at most
16 KiB, the exchange has a five-second deadline, and at most 16 exchanges run concurrently. The manager
can query availability but never choose a command, UID, PID or cgroup.

## Launch gate

`ora_process_runtime::spawn_linux_helper_workload` is a building block for trusted helper code, not a Run
API. It reloads the root-owned configuration and accepts only an existing `cgroup_root/scope/run` target;
it creates no directories, keeps no journal and does not deduplicate calls, so a future lifecycle owner
must own admission and cleanup responsibility.

It checks root-controlled migration ancestors, empty direct membership, an empty unfrozen Run subtree and
a writable membership file. Before exec the forked child writes `0` to the pinned membership descriptor,
starts a session, marks non-stdio descriptors close-on-exec, sets `no_new_privs`, clears supplementary
groups, sets all real, effective and saved GIDs and UIDs, clears capabilities and changes to the cwd as
the workload; any failed step aborts exec. Only the supplied environment is used and the returned `Child`
owns three pipes, so the caller drains output, reaps the child and enforces descendant policy.

The privileged acceptance test is explicitly ignored and runs only in a disposable,
administrator-provisioned environment: set `ORA_PROCESS_HELPER_TEST_CONFIG` and run
`cargo test -p ora-process-runtime --test helper_launch_privileged -- --ignored` as root. It creates and
removes only fresh Scope and Run directories under the configured root.
