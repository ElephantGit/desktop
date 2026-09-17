# Process host creation journal

English | [中文](process-host-state.zh.md)

Linux `ora_process_runtime::HostState` owns durable host creation intent and one-shot
[independent guardian bootstrap](process-guardian.md), not a production host app or Run launcher. It implements part of the approved
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
directory, which is preserved and rejected on recovery. Automatic repair is not provided; only the
exact version-1 and version-3 journals have the migrations described below.
Do not delete lock files, replace the directory while owned, or remove records to bypass a failure.

## Durable facts, not launch authority

The host holds the original `host.lock` until its SQLite connection closes. Recovery first takes that
lock nonblockingly, then performs a read-only journal compatibility check, and only then commits a
new host incarnation. Contention is an error, not permission to replace a lock or endpoint.

The version-2 journal identifies itself with application ID `0x4f524148` and `user_version=3`, and
checks its exact schema, integrity and persisted identities. It stores a positive host epoch with a
host instance ID, plus each Scope's original guardian instance, creating host binding and
`intent_recorded` phase. Epoch overflow is rejected; recovery never rewrites an intent's creating host.
Existing scope directories must have canonical IDs, private directory metadata and matching host
intent. Their internal journals and endpoints are not inspected or managed by this slice.

`record_scope_intent(scope)` commits a new original intent or returns the existing one unchanged.
`scope_intent(scope)` queries that responsibility; absence is not permission to recreate a guardian.
This registration call produces no Scope directory, guardian journal, process, credential or Ready fact.
A pre-existing Scope path without an intent blocks registration and is preserved.

`start_guardian(scope, executable)` requires that intent and an explicitly supplied trusted executable.
It first commits a `launch_unknown` record, then creates the private Scope directory,
acquires its lock and execs the guardian. Every subsequent error or cancellation consumes the attempt;
even a proven exec failure cannot authorize a second launch. `guardian_access(scope)` restores the
original discovery material after host recovery; it neither launches nor transfers control authority.
Guardian readiness is queried separately through `ora-process-client`.

Recovery accepts the exact version-1 intent-only schema and atomically adds the launch table while
advancing host identity. Original intents, paths and lock inode remain unchanged. That old version
could not launch guardians; it has no launch records to infer or backfill. The exact version-2 schema is migrated only when no Scope directory exists. Existing legacy scopes
block migration before any authority write or credential removal; their guardian may still need the
original token protocol. Keep a compatible old host for them, or use a separate new state directory;
never delete a Scope to bypass this check. For eligible journals, the obsolete credential column is removed while all
consumed launch attempts are preserved. Historical SQLite free pages and backups are not securely
erased. Unknown schemas fail closed, and old binaries reject version 3 rather than resetting it. Host recovery never writes guardian.sqlite.

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
exhaustion, conflict repair and the exact version-1 and version-2 upgrades, including unchanged legacy tokens and epochs on rejection. Crash fixtures override child HOME and cwd while using the same
explicit state path. Tests use a private temporary fixture under the test user's home; production
code does not derive a path from that environment variable.

Real-app bootstrap, refusal, launcher-kill and discovery evidence is documented under
[guardian bootstrap](process-guardian.md), including durable host takeover and guardian-side Run acceptance. Host-side Run intent/projection,
production host composition and platform cleanup remain unfinished; Controller authorization is deferred. No ADR is marked implemented.
