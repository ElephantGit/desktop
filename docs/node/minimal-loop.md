# Node minimal loop: clone a specified repository and branch

English | [中文](minimal-loop.zh.md)

## Current direction

As of 2026-09-18, the minimal loop changes from creating/deleting task worktrees under an existing
Main Workspace to **cloning a specified repository at a specified branch**. The local
Desktop–Controller–Node split remains: the caller specifies the repository and branch, Controller
coordinates, Node performs the clone in its execution environment, and the caller can observe the result.

This records direction, not an available clone API. The minimal execution contract is approved;
clone request framing and input validation are implemented. Results, capabilities and storage remain to be connected.

## Existing foundations and gaps

- The [standalone Node](runtime.md) provides startup recovery, shutdown and injected data paths;
  Linux host/guardian can run managed Git.
- [Durable Worktree execution](persistence/worktree-execution.md) remains implemented, but is no longer
  the first end-to-end objective.
- Current capability negotiation, terminal results and Node storage contain Worktree-specific constraints.
  Clone needs explicit adaptation, not an existing Main Workspace requirement or a disguised EnsureWorktree.
- Node-facing IPC, Controller coordination and the new Client entry are not connected. Existing passing
  tests do not establish a clone loop.

Stable execution identities, durable responsibility before dispatch, process handoff, queryable results
and acknowledgement after durable takeover remain reliability principles. Trust infrastructure and Strong
containment remain deferred; private Git access uses trusted, noninteractive Node deployment credentials.
Existing Backend writers, Worktree records and filesystem layouts remain unchanged.

## Approved boundaries and remaining design

| Topic           | Approved policy / remaining work                                                                                                           |
| --------------- | ------------------------------------------------------------------------------------------------------------------------------------------ |
| Input           | HTTPS and explicit SSH, deployment credentials, explicit branch; return the actual fetched commit                                          |
| Local resources | Node allocates an exclusive destination under an injected root; preserve failed/unknown residue                                            |
| Git scope       | Full single-branch history and checkout; hooks disabled, no recursive submodules or LFS downloads                                          |
| Coordination    | Replay the original execution; concrete messages, storage migration, Controller takeover and Client entry still need implementation design |

The accepted discussion about disabling Worktree-management hooks and repository-wide gates for unknown
process cleanup has not been implemented. It is neither current code behavior nor a complete clone failure model.

## Specification entry

The [clone root decision](../../specs/decisions/node/repository/0-clone-selected-repository-branch.md)
and [minimal execution contract](../../specs/decisions/node/repository/20260918-minimal-clone-execution-contract.md)
were approved on 2026-09-18, confirming scope, input, destination, content and recovery policies. Core verification obligations
have partial input-codec evidence; execution evidence remains Missing. This change performs no clone,
user-directory migration, IPC integration or Backend writer cutover.
