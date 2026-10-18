# Node Plugins

English | [中文](plugins.zh.md)

In Cloud mode, Cloud freezes a Workspace's plugin plan, the Controller dispatches it unchanged, and the
Node installs or removes the plugins in its own home. The Controller neither resolves marketplace
versions nor creates plugin Substrate effects. The decisions are
[plugin installs as Node executions](https://github.com/ora-space/specs/blob/main/decisions/controller/plugin/0-plugin-installs-as-node-executions.md),
[Node installs plugins into the Workspace home](https://github.com/ora-space/specs/blob/main/decisions/node/plugin-runtime/0-node-installs-plugins-into-workspace-home.md)
and the [bounded deployment download timeout](https://github.com/ora-space/specs/blob/main/decisions/node/plugin-runtime/20261010-bounded-deployment-download-timeout.md).

## Dispatch

The Node must advertise `plugin_install`. Cloud deployments send `ControlledPlugins` carrying
`InstallPlugins` or `RemovePlugins` and an execution permit; the Node checks authority at admission and
again when execution starts, persists the original input, and runs downloads and extraction on a
separate thread so heartbeats and queries stay live. Private local IPC also accepts bare commands while
runtime control has never been enabled for that home. The Controller advances the Workspace operation's
`plugin` step once Cloud accepts the terminal evidence, including individual item failures; a failure of
the whole execution defers the step with `PLUGIN_EXECUTION_FAILED` for retry.

## Installation

Packages live under `node.home_directory` at `plugins/installed/<namespace>/<name>/<version>/`.
Downloads and extraction use `plugins/.node-installs/install-*`, outside discovery. Installation reuses
`ora-plugin-manager` validation and checks the planned SHA-256, identity, version and target before
publishing. It never starts plugin code.

- A valid exact version already installed succeeds without downloading. An upgrade retires old versions.
  Removing an absent plugin succeeds, and removal keeps plugin data and configuration. Existing symlink
  directories are refused.
- Each attempt has a time limit (60 seconds by default) and at most three attempts handle transient
  failures; connection establishment is limited to 10 seconds and artifacts to 512 MiB. A failed
  attempt restarts the transfer.
- The per-attempt limit can be raised for slow links with `"plugins": { "download_timeout_seconds": 600 }`
  (an integer from 10 to 1200; invalid values fail startup). The total budget is three times the limit
  plus 70 seconds. Remote commands cannot select this policy, and it changes neither TLS validation nor
  the frozen URL and SHA-256 checks.

`PluginInstaller::catalog()` supplies the `DirectoryPluginCatalog` that [Agent sessions](agent-sessions.md)
share: a session takes a use lease and then resolves the exact version, and replacing or removing a plugin
returns `plugin_in_use` while any session holds a lease on it.

## Persistence and recovery

Plugin executions and their outbox live in the Node database (schema v7). Reusing an identity with
different input is rejected. The terminal result and its single sequence-1 event commit atomically;
queries and duplicate commands consume nothing, and only an exact ACK clears the event while the result
stays queryable. The Controller commits Cloud's takeover before acknowledging an event and never
acknowledges evidence learned only from a query.

Startup removes this executor's temporary directories. Unfenced local unfinished inputs can rerun. After
a Cloud Node restart the old incarnation's permit is invalid, so retained unfinished executions settle as
`interrupted` and Cloud registers a new execution with new authority. Completed but unacknowledged events
replay with their original identity.
