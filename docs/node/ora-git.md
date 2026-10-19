# ora-git

English | [中文](ora-git.zh.md)

`ora-git` (`apps/ora-git`) is the command an Agent uses inside its sandbox to fetch from and push to
the platform Git Server. It never runs Git against the network and holds no credential: it turns one of
a fixed set of verbs into a one-line JSON request on the session's Unix socket, and the Node runs the
request with its own git-access identity. The decision is
[clone from a Git Server snapshot and ora-git](https://github.com/ora-space/specs/blob/main/decisions/node/repository/20261016-clone-from-git-server-snapshot-and-ora-git.md);
what the Git Server lets each execution write is in
[snapshot clone input and execution ref grants](https://github.com/ora-space/specs/blob/main/decisions/cloud/controller-integration/20261016-snapshot-clone-input-and-execution-ref-grants.md).

## Deployment

```json
"agent": { "deno_path": "/opt/ora/bin/deno", "ready_timeout_ms": 30000, "ora_git_directory": "/run/ora-git" }
```

- `agent.ora_git_directory` enables `ora-git`; without it, or without `git_access`, sessions get no
  socket. The Node owns the directory (`0711`, traversable but not listable by others) and empties it
  when it starts.
- The Node image installs the binary at `/usr/local/bin/ora-git` on the workload `PATH`, see
  [Container images](../operations/container-images.md). The socket client is Unix-only; built for
  another platform, `ora-git` answers `internal`.

## Sessions and sockets

Each running Agent session gets its own socket with a random name in that directory, and its path
reaches the agent plugin and the processes it spawns as `ORA_GIT_SOCKET`. A session whose socket cannot
be opened ends as `agent_failed{ora_git_unavailable}`.

- A request belongs to the session whose socket it arrived on, never to anything it declares: it carries
  no execution, Workspace or repository. The peer (`SO_PEERCRED`) must be the workload user, otherwise
  the answer is `forbidden`.
- Requests fail as `no_session` once the Node is stopping, an `EndSession` was accepted (before the
  session acts on it), or the session has ended.
- One request per session runs at a time; a concurrent one gets `busy`.
- When the session leaves the runtime, the socket refuses new requests at once but waits for a request
  whose Git is already running, so nothing the session asked for still writes its checkout or the Git
  Server once the session is recorded as ended and delivery may start. The socket file is then removed.
  A client whose connection closes without an answer reports `no_session`.

Requests reach the Git Server with the session execution's `Ora-Execution-Id`; the Git Server decides
from Cloud's current state what that execution may read and write and stays authoritative.

## Verbs

```text
ora-git info | list | fetch upstream | fetch workspace <workspaceId> [<branch>] | push [<branch>]
```

| Verb | What it does |
|---|---|
| `info` | Prints this Workspace's id, branch and base commit and where fetched refs land; no network access |
| `list` | Lists the Binding's upstream branches and every Workspace branch on the Git Server, annotating Workspaces with the kind, Issue, run, run status and title from the session's Workspace directory, as Cloud froze it when the session started |
| `fetch upstream` | Fetches the Binding's upstream branches into `refs/remotes/ora/upstream/` |
| `fetch workspace <id> [<branch>]` | Fetches one Workspace's branches, or one of them, into `refs/remotes/ora/workspaces/<id>/` |
| `push [<branch>]` | Pushes the local branch (default: the Workspace's own branch) to `refs/heads/workspaces/<own id>/<branch>` |

Fetches write only `refs/remotes/ora/**`; local branches, HEAD, the index, working files and repository
configuration are preserved. The command line accepts no URL, refspec, option or force flag, and a branch
argument is a plain branch name (no `HEAD`, no leading `-`, no `:`, `+` or `*`, at most 200 bytes).
Progress lines go to stderr and the result to stdout.

## Pushing

A push writes only this Workspace's branch of the same name. The old value it declares
(`--force-with-lease`) comes from the Node ledger (`workspace_branch_heads`), recorded after this
checkout's own successful pushes and fetches of its own Workspace; Agent-writable refs such as
`refs/remotes/ora/**` never supply it.

- If the ledger's value is known locally and the pushed commit does not contain it, the push is refused
  as `not_fast_forward` before anything is sent.
- A stale lease is `conflict`, unless the Git Server already holds the pushed commit (a replay of a push
  whose answer was lost), which succeeds.
- A refused update is `denied`.

After a successful push the ledger and `refs/remotes/ora/workspaces/<own id>/<branch>` record the new
value.

## Exit codes

Failures print `ora-git: <code>: <hint>` on stderr; hints name only refs and commits.

| Code | Exit | Meaning |
|---|---|---|
| `usage` | 2 | Malformed command line or request |
| `no_session` | 3 | No running session behind the socket |
| `forbidden` | 4 | The caller is not the workload user |
| `busy` | 5 | Another request of the session is running |
| `not_found` | 6 | No matching ref or local branch |
| `conflict` | 7 | The Workspace branch moved since this checkout last saw it |
| `not_fast_forward` | 8 | The pushed commit does not contain the branch's last known value |
| `denied` | 9 | The Git Server refused the request |
| `unavailable` | 10 | The Git Server could not be reached |
| `internal` | 11 | The Node failed locally; the cause is in the Node log |
