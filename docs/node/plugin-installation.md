# Node plugin installation

English | [中文](plugin-installation.zh.md)

Cloud freezes the Workspace plugin plan, Controller dispatches it, and Node executes it. Controller
registers the operation snapshot's `plugin_input` unchanged. It neither resolves marketplace versions
nor creates plugin Substrate effects. The Node handshake must advertise `plugin_install`. Controller
advances the plugin step after Cloud accepts terminal item evidence, including individual failures;
a whole-execution failure defers with `PLUGIN_EXECUTION_FAILED` for retry.

## Execution and layout

Cloud deployments send `ControlledPlugins`, containing `InstallPlugins` or `RemovePlugins` and an
execution permit. Node checks authority at admission and at execution start, persists the original
input, and runs downloads and extraction on a separate thread so heartbeats and queries remain live.
Private local IPC also accepts bare commands while runtime control has never been enabled for that
home; enabling it durably closes that fallback.

Packages live under the explicit `node.home_directory` at
`plugins/installed/<namespace>/<name>/<version>/`. Downloads and extraction use
`plugins/.node-installs/install-*`, outside discovery. Installation reuses `ora-plugin-manager`
validation, verifies the planned SHA-256, identity, version and target, then publishes the package.
Installation never starts plugin code. Transfers have time and size limits and at most three attempts
for transient failures. A valid exact version succeeds without downloading again. Successful upgrades
retire old versions. Removing an absent plugin succeeds; removal preserves plugin data/config.
Existing symlink directories are refused.

`PluginInstaller::catalog()` supplies a `DirectoryPluginCatalog`. Session hosts must share this
instance, acquire a use lease, then resolve the exact version. Replacement and removal return
`plugin_in_use` while any session leases that plugin. This change supplies the catalogue and leases;
wiring Agent session execution into the production control channel remains a later part of task C.

## Persistence and recovery

SQLite schema v7 adds plugin executions and an outbox while retaining existing worktree/clone
identities, results and events. Reusing an identity with different input is rejected. The terminal
result and its single sequence 1 event commit atomically. Queries and duplicate commands do not
consume events. Only an exact ACK clears the outbox; the result remains queryable. Controller commits
Cloud takeover before acknowledging an event, and never acknowledges evidence learned only by query.

Startup removes this executor's temporary directories. Unfenced local unfinished inputs can rerun.
After a Cloud Node restart, the old incarnation's permit is invalid: retained unfinished executions
settle as `interrupted`, and Cloud/Controller registers a new execution with new authority. Completed
unacknowledged events replay with their original identity. This follows current runtime control and
narrows the earlier ADR's unconditional restart-and-rerun rule.

## Verification

Relevant interfaces are exercised by `apps/ora-node/tests/plugins.rs` (real archives and local HTTP),
`apps/ora-node/tests/standalone/repository_plugins.rs` (real Node process and IPC restart replay),
`crates/node-db/src/tests/plugin.rs` (transactions, restart and authority), and
`apps/ora-controller/tests/workspaces/plugins.rs` (operation driving through fake Cloud gRPC and Node
WebSocket services).

This scope verifies the desktop plugin execution chain. Real Cloud database projection and the
container Workspace volume still require the cross-repository M1 integration run.
