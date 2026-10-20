# Node

English | [中文](node.zh.md)

`ora-node` (`apps/ora-node`) is the Linux executable that runs work inside an execution environment:
in Cloud mode, one Node runs in each Workspace sandbox. It owns one explicitly configured Node
database, executes clones, plugin installations, Agent sessions and Revision deliveries, recovers
unfinished executions after a restart, and runs every Git command through the
[process host and guardian](../process/process-runtime.md). It accepts commands only from its
Controller over the [control session](control-session.md); configuration files and stdin are not
command channels.

## Deployment

Build with `cargo build -p ora-node -p ora-process-host -p ora-process-guardian`, start the
[process host](../process/host.md), then run `ora-node /absolute/path/node-config.json`. A minimal
configuration:

```json
{
  "node": { "home_directory": "/home/alice/.ora/node", "identity": "Discover", "repositories": [] },
  "process": {
    "host_directory": "/home/alice/.ora/process",
    "expected_uid": 1000,
    "git_program": "/usr/bin/git",
    "environment": { "PATH": "/usr/bin:/bin", "HOME": "/home/alice" },
    "command_timeout_ms": 30000,
    "cleanup_timeout_ms": 5000,
    "shutdown_grace_ms": 2000
  },
  "timezone": "Asia/Shanghai",
  "recovery_interval_ms": 1000
}
```

Further sections enable capabilities: `clone` ([Repository clone](repository-clone.md)), `control`
([Control session](control-session.md)), `agent` ([Agent sessions](agent-sessions.md)), `plugins`
([Plugins](plugins.md)) and `git_access` ([Revisions](revisions.md#git-server-access-material)),
which clone, Revision delivery and [ora-git](ora-git.md) require; a Cloud-mode Node refuses to start
without it.
`process.workload_uid` runs Git workloads and agents as a separate user.

- `"Discover"` keeps an existing NodeId or generates the first one; `{ "Require": "id" }` enforces a
  registered identity. Each start generates a fresh NodeIncarnationId.
- Both state directories are absolute and injected, never derived from `HOME` (the `HOME` in
  `environment` configures Git only). New directories are private; release builds reject existing
  non-private ones, while debug builds skip permission-bit checks. Both reject wrong owners, symlinks
  through trusted paths and aliases of host state.
- `node.repositories` holds explicit `RepositoryBinding`s (repository reference, Main Workspace
  identity and path, authorized root, worktree root) for [worktree execution](worktree-execution.md).
  Nothing is cloned or inferred from the cwd.

## Execution and recovery

All Git runs use the configured executable and environment through the host and guardian. Read-only
preflight runs share bounded read scopes. Before each mutation the Node commits its execution, host,
Scope and Run association in its own database, while the host and guardian keep their own intent and
facts; a lost reply never authorizes a fresh attempt. Output is bounded and volatile, and truncated,
failed or missing output is never parsed as complete Git facts.

On recovery the Node seals each unfinished mutation Scope and waits for observed closure before
examining or repairing its resources. An unavailable guardian or unverified cleanup leaves that
execution `Unknown` with its reservations intact, while non-conflicting work can still proceed.

The guardian pins the requesting Node's process identity before exec, so the Node exiting triggers Run
cleanup even if the host is disconnected. This is rootless best effort, not strong containment: an
escaped descendant or a dead guardian can prevent verified cleanup, and owner liveness only triggers
cleanup.

`SIGTERM` and `SIGINT` close admission, give the active Git command its grace, then request cleanup.
Shutdown verifies pending Scopes and reports failure rather than claiming unverified cleanup; business
reconciliation may remain for the next start.

## Storage

The database is always `home_directory/ora-node.sqlite3`. Node owns business execution, resource and
event records; the host and guardian own separate journals.

- Opening holds an exclusive OS lease on the sibling `ora-node.sqlite3.lock` for the database's
  lifetime, never on the database file, whose SQLite locks would otherwise collide with a whole-file
  lease on macOS and Windows. The lease excludes another Node owner but does not prove an older Git
  process stopped.
- SQLite uses its default rollback journal with `synchronous=FULL`. The database has application id
  `0x4f52414e`. Opening checks identity, integrity, exact table and index definitions and foreign
  keys before anything else; empty files, foreign databases, unsupported versions, directories and
  corrupt databases are rejected and never rebuilt.
- Older exact schemas migrate transactionally, preserving executions, results and pending events;
  older binaries refuse newer versions instead of reading them.

| Version | Adds |
|---|---|
| 1 | `node_metadata`, `executions`, `resources`, `outbox` (Worktree executions) |
| 2 | `managed_executions`, `process_attempts`: process associations |
| 3 | `execution_identities`, `clone_executions`, `clone_outbox`, `process_outcomes` |
| 4 | `controller_binding`, `execution_controllers`: the deployment-bound ControllerId |
| 5 | `process_terminations`: Runs a signal ended after confirmed cleanup |
| 6 | Runtime bindings, enforcement and execution permits |
| 7 | `plugin_executions`, `plugin_outbox` |
| 8 | `node_executions`, `execution_events`, `session_commands`: Agent sessions |
| 9 | `revision_deliveries`, `revision_outbox` |
| 10 | `workspace_branch_heads`: the own Workspace branch values `ora-git` pushes declare |
| 11 | `session_restores`, `restore_process_attempts`: restore admission, original identities and stop clocks |

Every execution family shares `execution_identities`, which prevents key collisions across families.
Completion commits resource facts, the terminal result and its original event together; status reads
never acknowledge. An acknowledgement must match Node, operation, execution and sequence and removes
only the delivery record, while results and deduplication survive. Migration never invents process
evidence for legacy direct-Git executions, so such an in-flight execution stays blocked. Environment
values in a stored `RunSpec` are private data, never logs.

`WriteGuard` exposes transaction failure points for real SQLite fault tests; `cargo test -p
ora-node-db -p ora-node` runs them.

Restore Git admission and its original command intent commit together. A session terminal requires original stop evidence; bound expiry retains named residue and never deletes identity. Pre-journal restores remain `legacy_unknown`: old terminals are retained as history but do not release checkout/runtime admission fences. See [Revision recovery](revisions.md).
