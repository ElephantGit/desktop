# Revisions

English | [中文](revisions.zh.md)

When an Agent session ends, the Node saves its checkout and history as a **Revision**: it pushes the
final commit to a delivery ref on the platform Git Server and uploads only the sealed session JSONL to
object storage, through a grant Cloud signs. A later run of the same Issue restores that Revision from
the Git Server before its agent starts. No bundle is generated, uploaded or downloaded. The decisions
are the [internal snapshot root decision](https://github.com/ora-space/specs/blob/main/decisions/node/revision/0-internal-snapshot-and-incremental-bundle-upload.md),
whose snapshot commit still applies, and
[push the delivery ref and restore from the Git Server](https://github.com/ora-space/specs/blob/main/decisions/node/revision/20261017-push-delivery-ref-and-restore-from-git-server.md)
(`approved`; it becomes `implemented` after end-to-end acceptance), which replaces its bundle upload and the bundle restore of
[restore prior Revision before session](https://github.com/ora-space/specs/blob/main/decisions/node/revision/20261010-restore-prior-revision-before-session.md).
The Controller's side is in [Agent relay](../controller/agent-relay.md), the grant contract in
[Cloud contract](../controller/cloud-contract.md#revision-upload-checksums).

The Node advertises `revision_delivery` when it has both the `clone` and the `git_access` sections,
because delivery runs Git in clone checkouts with clone's hardened Git environment and pushes with the
git-access identity, and `revision_restore` exactly when it advertises both `agent_session` and
`revision_delivery`.

## Admission

- `controlled_deliver_revision` is the production entry: its runtime permit must match this Node,
  incarnation, operation and execution, checked at admission. A bare `deliver_revision` is accepted
  only on private IPC in a home that never enabled runtime control.
- The input names the ended session, its clone execution, the base commit, a local Revision ref under
  `refs/ora/revisions/`, the Git Server `git_repository_url`, a `delivery_ref` under
  `refs/ora/deliveries/`, the `workspace_ref` and the history object key. The Workspace ref must equal
  the clone's.
- The input is persisted before any work and the delivery recorded as `Running`. Repeating the same
  input only reports state; different input under the same identity is a protocol error.
- Deliveries live in `revision_deliveries` with their single terminal event in `revision_outbox`,
  separate from sessions so session recovery never sees them.

## Preparation

Preparation happens once:

1. The session must have its terminal result and no live actor (`failed{session_not_settled}`), its
   checkout must still be at the delivery's base commit (`failed{checkout_unavailable}`), and its JSONL
   must exist (`failed{history_unavailable}`).
2. The sealed JSONL is copied into `<home>/revision-deliveries/<sha256(execution)>/history.jsonl`.
3. A scratch index in the checkout's Git directory snapshots the worktree (`read-tree HEAD`, `add -A`,
   `write-tree`), as the workload user. If the tree equals `HEAD^{tree}` the final commit is `HEAD`;
   otherwise `commit-tree` adds one commit authored by the session's Git identity and committed by
   `Ora <revision@ora.invalid>`. Hooks, fsmonitor and signing are disabled, and the real index,
   worktree files and branches are not touched. `update-ref` points the local Revision ref at the final
   commit.
4. A final commit equal to the base, or for a resumed session to the prior Revision's final commit, is
   declared `unchanged`; any other is `delivered`. Either way the delivery pushes its own delivery ref.
5. The final commit, the history's size and SHA-256 and the declaration are written to the ledger as the
   frozen plan before the first network write. A restart before that prepares again; after it, the same
   plan is reused.

## Push and upload

Git that uses the Node's git-access identity never runs in the checkout. Each push uses a private
transport repository under `<repository_root>/.ora-transport/` that borrows only the checkout's object
store (nested alternates and shared directories are refused, and alternate ref discovery is disabled).
The certificate, key, CA and the delivery execution's `Ora-Execution-Id` header are given only to each
HTTPS command, so checkout configuration cannot redirect it or install credential helpers.

1. Push the frozen commit to `delivery_ref`. Creating it, or finding it already at that commit,
   succeeds; a refused update is `failed{push_rejected}`, and exhausted network or 5xx failures are
   `failed{push_failed}`.
2. Try to fast-forward `workspace_ref` to the same commit, declaring the value the Node ledger last saw
   there. A replay succeeds; a stale value, a refusal or exhausted failures are only logged and never
   fail the delivery.
3. Upload the history: without a valid grant (none yet, expiring within 5 s, refused with 403, or lost by
   a restart) the Node sends `upload_grant_needed` with the frozen SHA-256 of the history, repeats it
   every 2 s and again at once on every new connection, and the delivery stays `Running`. The PUT
   streams the frozen file with every grant header unchanged (including `If-None-Match: *` and
   `x-amz-checksum-sha256`) and never follows redirects. 2xx and 412 count as uploaded; three failed
   attempts end the delivery as `failed{upload_failed}`. Grants live only in memory and are never logged.

Transient Git Server failures get three attempts with 5 and 30 second pauses; 401, 403 and other 4xx
answers except 429, and refused ref updates are not retried and fail as `push_rejected`. Each push attempt may run 30 minutes, independent of `network_timeout_ms`, and
aborts below 1 KiB/s for a minute. Cloud verifies the delivery ref and the history outside its
transaction before registering the Revision.

The terminal `revision_result` (sequence 1) is replayed until acknowledged; the frozen files are removed
once it is durable and leftovers are swept at the next start. Shutdown, Controller disconnection and
cancellation never write a failure; the delivery resumes from its frozen plan after restart.

## Restoring a prior Revision

A session with `prior_revision` (its identity, final and base commits, `git_repository_url` and
`delivery_ref`) restores it after resolving the checkout and checking the plugin, and before the checkout
is handed to the workload user or any plugin starts. The repository must be the one the checkout was
cloned from.

1. As the Node, with the session execution's identity, fetch `delivery_ref` into a private transport
   repository that borrows the checkout's objects, negotiating only from the clone's base commit. The
   fetched commit must equal the prior final commit.
2. As the workload user, take the objects from the transport repository into the checkout and move the
   Workspace's local branch to the prior final commit. The clone's base, the repository configuration
   and `refs/remotes/ora/**` are untouched.
3. If the prior base commit is not in the history of the current snapshot, one fixed English note naming
   the restored commit, its base and the branch, and recommending `ora-git fetch upstream`, is appended to
   the first turn.

The fetch and the import share one 5-minute deadline, retries included. A missing delivery ref, a 403
or another 4xx other than 401 and 429 ends the session as `agent_failed{prior_revision_base_unavailable}`:
the prior Revision's Git content is out of reach for good. A 401 only says the Node's own git-access
identity did not authenticate, so it ends the session as `agent_failed{prior_revision_unavailable}`,
which a later run may retry, without a retry within the deadline since the same material cannot pass.
Any other fetch, verification or checkout failure, or the deadline, also ends it as
`agent_failed{prior_revision_unavailable}`. No plugin process is
started in either case. A Node stop or an `EndSession` during the restore stops it before its next Git
command and waits for the running one, so the session ends only once the restore no longer writes the
checkout. A restart during a restore ends the session `interrupted`. Transport repositories are removed
after use and swept at startup.

## Git Server access material

A deployment that enables the platform Git Server adds a top-level `git_access` section with absolute
`ca_cert`, `client_cert` and `client_key` paths for a client certificate issued for the runtime's
tenant, Workspace and generation, separate from the WSS and model-access identities. The entrypoint
keeps these files in the root-only `/run/ora`. The Node validates the material at startup and refuses to
start when it is unusable, and a Cloud-mode Node (`control.target` set) without the section refuses to
start with `git_access_required`. The certificate carries identity only: the Git Server decides what each
request may read or write from Cloud's current state for the execution in its `Ora-Execution-Id`
header, and execution inputs supply the server address. The same material serves
[clone](repository-clone.md), delivery, restore and [ora-git](ora-git.md).

## Limits

- Agent plugins run outside host Scopes, so "process scope closed" means the session's result is written
  and the session runtime stopped its plugin tree.
- Delivery Git Runs are host-managed but not journaled as business mutations; an interrupted preparation
  simply prepares again.
- Repository configuration can still select clean or smudge filters that `git add` runs, under the
  workload identity like the agent itself.
- Parsing the borrowed checkout objects in the transport repository has no resource limits of its own.
- A `revision-restores/` directory left in the Node home by an older Node is not removed.
