# Process host creation journal

English | [中文](process-host-state.zh.md)

Linux `ora_process_runtime::HostState` now owns durable **host creation intent**, not a running host
or guardian. It is a preparatory slice of the approved
[guardian bootstrap decision](../specs/decisions/node/process/recovery/20260917-rootless-guardian-bootstrap-and-reconnect.md).
It requires no root, helper, cgroup delegation or service installation. Existing Git/plugin entry
points and application database policy are unchanged.

## Explicit location and ownership

The caller supplies an absolute, dedicated `state_dir`. `HostState` never reads HOME or business cwd.
Application composition must choose this directory from its explicitly injected Node/home location;
that production composition is not implemented yet.

- `HostState::create(&state_dir)` requires an absent directory, including rejecting an existing empty
  directory. It creates only `host.lock`, `host.sqlite`, SQLite sidecars and `scopes/`.
- `HostState::recover(&state_dir)` requires the existing stable lock, database and scopes directory.
  Missing files, unknown root entries, malformed identities and incompatible journals fail without
  resetting state. It never retries creation after a recovery error.
- Directories are owner-private; files are owner-private regular inodes with one hard link.
  Symlinks and group/other-writable ancestors are rejected using shared `ora-utils::path` checks.
  Existing permissions are not changed. Root and the selected UID remain trusted; this is not
  isolation from malicious same-UID code.
- The initial filesystem allowlist is the ext family, XFS and Btrfs. Network, memory, overlay and
  unknown filesystems are rejected. Filesystem classification alone does not certify mount options,
  storage hardware or power-loss behavior. Local tests currently exercise ext-family storage only.
- The full canonical Scope ID and `control.sock` suffix must fit Linux's pathname socket limit.
  Long paths are rejected, not shortened or redirected. A group-writable checkout or `/tmp` is not
  a supported state parent.

Creation is intentionally fail-closed: interrupted initialization can leave a partial dedicated
directory, which is preserved and rejected on recovery. Automatic repair/migration is not provided.
Do not delete lock files, replace the directory while owned, or remove records to bypass a failure.

## Durable facts, not launch authority

The host holds the original `host.lock` until its SQLite connection closes. Recovery first takes that
lock nonblockingly, then performs a read-only journal compatibility check, and only then commits a
new host incarnation. Contention is an error, not permission to replace a lock or endpoint.

The version-1 journal identifies itself with application ID `0x4f524148` and `user_version=1`, and
checks its exact schema, integrity and persisted identities. It stores a positive host epoch with a
host instance ID, plus each Scope's original guardian instance, creating host binding and
`intent_recorded` phase. Epoch overflow is rejected; recovery never rewrites an intent's creating host.
Existing scope directories must have canonical IDs, private directory metadata and matching host
intent. Their internal journals and endpoints are not inspected or managed by this slice.

`record_scope_intent(scope)` commits a new original intent or returns the existing one unchanged.
`scope_intent(scope)` queries that responsibility; absence is not permission to recreate a guardian.
No Scope directory, guardian journal, launch ticket, process, credential or Ready fact is produced.
A pre-existing Scope path without an intent blocks registration and is preserved.

The journal independently enables and verifies WAL plus `synchronous=FULL`; the linked SQLite mainline
version must include the WAL-reset fix (at least 3.51.3). Transactions commit before returning new
facts; containing directory entries are also synced. A filesystem error after commit can therefore
leave an accepted record even when the call reports failure: query the original identity, never infer
non-acceptance from the error. SQLite's durability semantics are described in its
[WAL](https://sqlite.org/wal.html) and [synchronous](https://sqlite.org/pragma.html#pragma_synchronous)
documentation. Physical power-loss durability remains unverified.

## Verification and remaining work

`cargo test -p ora-process-runtime --test host_state` exercises concurrent creation, lock contention,
deduplication across restart, unchanged lock identity, lost caller state after external SIGKILL,
missing/foreign files, path limits, permissions, links, version/schema/identity corruption, epoch
exhaustion and conflict repair. Crash fixtures override child HOME and cwd while using the same
explicit state path. Tests use a private temporary fixture under the test user's home; production
code does not derive a path from that environment variable.

The tests do not establish guardian launch ordering or survival. Scope lock qualification/handoff,
bootstrap credentials, guardian journal/app, authenticated Ready discovery, host-session fencing,
Run acceptance and platform cleanup remain to be implemented. No ADR is marked implemented.
