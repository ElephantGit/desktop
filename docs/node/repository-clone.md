# Repository Clone

English | [中文](repository-clone.zh.md)

A Node clones a specified repository at a specified branch into a directory it allocates, and reports
the actual fetched commit. Clone is its own execution family with its own records; it needs no
existing Main Workspace. The decisions are the
[clone root decision](https://github.com/ora-space/specs/blob/main/decisions/node/repository/0-clone-selected-repository-branch.md),
the [minimal clone execution contract](https://github.com/ora-space/specs/blob/main/decisions/node/repository/20260918-minimal-clone-execution-contract.md)
and [a terminated attempt fails as interrupted](https://github.com/ora-space/specs/blob/main/decisions/node/repository/20260925-terminated-clone-attempt-fails-as-interrupted.md).

## Deployment

```json
{
  "clone": {
    "repository_root": "/home/node/repositories",
    "git_config": "/home/node/deployment/clone.gitconfig",
    "search_path": ["/usr/bin"],
    "ssh": { "kind": "disabled" }
  }
}
```

- Provision the root and configuration before startup; the Node never creates or re-permissions them.
  Every path is absolute, owned by the current user or root and free of symlink components; release
  builds reject group- or other-writable paths. The root cannot overlap Node or process state or
  configured worktree checkouts.
- The filesystem must report birth time: device, inode and birth time identify the root and each newly
  created leaf. Missing or replaced ownership evidence stays `Unknown`; a database reservation alone
  never owns a directory. This is rootless, cooperative-user protection, not isolation from hostile
  code running as the same user.
- `git_config` may select CA certificates and non-interactive deployment credential helpers. Only
  configuration paths are copied into the Run environment; the ambient `HOME` is not inherited, and
  clone output is discarded so authentication diagnostics never reach the journal. Cloud deployments
  allow only non-secret CA trust here, see [Runtime control](../controller/runtime-control.md).
- SSH is off unless configured as
  `{ "kind": "configured", "program": "/usr/bin/ssh", "config": "/home/node/deployment/ssh_config" }`.
  That file selects identities and known hosts; the Node forces batch mode and strict host-key
  checking, so unknown hosts must be provisioned by the deployment.
- `process.command_timeout_ms` bounds local Git commands; network clone, fetch and push use
  `process.network_timeout_ms` (default 600000, allowed 1..1200000). An expired network budget still
  closes the original Scope and requires host and guardian cleanup evidence before reporting
  interrupted. Retry policy belongs to Cloud, which bounds Workspace clone attempts; the Node never
  invents one.

Embedding callers use `configure_clone`, `submit_clone` and `recover_clones`; the executable recovers
accepted clones at startup and takes new ones only from the control session.

## Execution

Each accepted request owns a new, exclusively created directory and at most one durable mutation Run.
Git performs a non-local, full-history, single-branch clone with no templates, no recursive submodules
and no LFS smudge or process filters; hooks and interactive prompts are disabled per command. A tag of
the same name cannot satisfy the requested branch. Verification requires a normal, non-shallow
checkout with independent objects, the requested source and branch, a HEAD matching the fetched branch
commit and clean tracked files.

Every attempt's end (Git exiting, cleanup after the stop grace or deadline, or recovery after restart)
first closes the original managed Scope and then classifies the host's view:

- an exit code with completed cleanup continues to verification;
- a signal termination with completed cleanup fails as `interrupted` and keeps the directory, because
  Git never reached a verdict;
- missing exit or cleanup facts stay `Unknown`.

A normal stop saves the interrupted result before exiting, and recovery after a kill reaches the same
result. Missing process evidence never justifies another Run. A reservation with no Run resumes only if
its persisted phase proves dispatch never happened. A known failure keeps its directory; a retry needs
new operation and execution identities and gets a different directory. Terminal replay always uses the
original durable result and event, offline or after user edits, without re-inspecting or recloning.

## Records

Clone records live in the Node database (schema v3 and later):

- `accept_clone` atomically stores the original command, a generated repository identity, the root and
  path, and the initial reservation; it never creates a directory. Replaying the same input returns the
  original target; changed input or reuse across families fails. A destination cannot overlap any
  active worktree target or retained clone target, and a failure never releases its reservation.
- Progress records `Reserved`, `DirectoryCreated` or `Dispatched`. Directory identity is frozen after
  creation and `Unknown` keeps its phase. The process journal starts the managed Run only after durable
  directory creation evidence.
- The terminal result and its event commit in one transaction and must match the original input,
  resource and path. A dispatched attempt needs exactly one observed ending and no uncleaned Runs: an
  exit code (`process_outcomes`) backs success or a Git failure, a signal termination
  (`process_terminations`) backs only `interrupted`, and success requires exit code zero.
- Queries never acknowledge; `pending_events` spans every family, and an acknowledgement removes only
  the delivery record while results, input and the reservation survive.
