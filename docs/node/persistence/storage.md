# Node local storage

English | [中文](storage.zh.md)

`NodeConfig.home_directory` is injected by the caller. Deployment will supply `~/.ora/node`;
unit tests supply temporary directories. The database is always `home_directory/ora-node.sqlite3`.
The library does not read `HOME` or start a process or IPC server.

Opening a database holds an exclusive OS file lock for its lifetime. SQLite uses its default
rollback journal and FULL synchronous writes. A new database receives application ID `0x4f52414e`
and schema version 1. Existing empty files, foreign databases, unsupported versions, directories
and corrupt databases are rejected without rebuilding them. The persistent NodeId survives
reopening; each Node runtime generates a fresh NodeIncarnationId. An explicit identity mismatch
fails initialization.

`ora-node-db` owns four tables: `node_metadata`, `executions`, `resources`, and `outbox`.
The complete command is stored separately from the resolved target, which freezes the canonical
binding, authorized roots, task path, branch and base commit. Unique operation/execution identities
prevent rebinding. Active resources reserve workspace, path and repository-local branch before Git
runs. Deletion references existing ownership and retains a tombstone after completion.

Guarded transitions preserve Accepted, Running, Unknown and Completed evidence. Completion commits
resource facts, the terminal result and its original event envelope together. Status reads have no
acknowledgement effect. An acknowledgement must match Node, operation, execution and sequence 1;
it removes only the delivery record. Results and execution deduplication survive acknowledgement.

`WriteGuard` exposes transaction failure points for real SQLite fault tests. Tests in `ora-node-db`
cover exclusive ownership, file preservation, deduplication, reservations, rollback, reopening and
acknowledgement. Run `cargo test -p ora-node-db -p ora-node`.

Opening also checks table/index definitions and foreign-key integrity; the schema identifier alone
cannot authorize an unknown structure. A definitive no-effect creation failure retires reservations
and releases active uniqueness while retaining execution deduplication and the failed result.
Inconclusive executions continue to hold their reservations.
