# Workspace Review

English | [中文](workspace-review.zh.md)

The Changes panel reviews Git changes in any Workspace checkout (a task worktree or a project's main
checkout) and stages, commits and pushes them. Use cases live in
`crates/application/src/workspace_diff`; the UI lives in `packages/app-shell/src/features/diff`.

## Operations

| Operation | Purpose |
|---|---|
| `getWorkspaceDiff` | A bounded unified patch for one scope |
| `getWorkspaceStatus` | Per-file staging status |
| `stageWorkspaceChanges`, `unstageWorkspaceChanges` | Stage or unstage paths; an empty list means all changes |
| `commitWorkspaceChanges` | Commit exactly the staged change set |
| `pushWorkspaceBranch` | Push the checked-out branch to `origin` |

Requests carry the `WorkspaceId`; the backend resolves the live checkout and never accepts a
frontend path or a `TaskId`.

## Scopes and baselines

`WorkspaceDiffScope` is `Branch`, `Unstaged`, `Staged` or `Committed`.

- A Workspace with a `Worktree` row (a task) has a fixed baseline: the commit the worktree was
  created from. `Branch` and `Committed` compare against it. Before committing or pushing, the
  handler verifies the checkout's path and branch against the recorded state.
- A Workspace without one (a project's main checkout) has no baseline, so only `Unstaged` and
  `Staged` apply, and writes go to whatever is currently checked out without verification.
- Requesting `Branch` or `Committed` without a baseline returns
  `workspace_diff_baseline_unavailable` before Git runs.

The patch is bounded before it enters memory: an oversized patch returns `workspace_diff_too_large`
and the discarded size stays out of the contract. A blank commit message returns
`workspace_diff_commit_message_blank`. Git and persistence failures keep their source chains and are
projected to `internal_error`. Pushing uses `GitIntent::Network` with prompts disabled, see
[Gitlancer](gitlancer.md).

Short-lived readers and writers hold a shared worktree-use lease, so a deletion that commits while a
diff or push is running cannot remove the checkout underneath it, see
[Tasks and worktrees](tasks-and-worktrees.md#deletion-and-durable-git-cleanup).

## Refresh

`state/data/workspace-review.ts` owns `refreshWorkspaceReview(queryClient, workspaceId)`. It
invalidates every diff scope and the staging status of that Workspace; active queries refetch
concurrently, inactive ones become stale, and other Workspaces keep their cache.

- Staging and commit mutations await this refresh inside the operation and keep the originating
  Workspace even if the view switches while the write is pending.
- Manual refresh, prompt completion cleanup and debounced agent file-change or turn-completion
  events use the same interface; event consumption never waits for network responses.
- Each refresh cancels earlier diff and status requests, including initial loads, before starting
  new ones, so a late response cannot replace the newer snapshot. Overlapping refreshes share one
  completion per `QueryClient` and Workspace until the latest round settles, including retries and
  offline pauses; superseded rounds cannot settle those waiters.
- A refresh failure stays a query error. It never turns a successful Git write into a failed
  mutation; the view shows the error and offers retry.

## Rendering large change sets

The panel splits the backend's patch into per-file slices and caches each parsed file, so a refresh
that rewrites a few files keeps every unchanged file's memoized rendering. Patches above about
512 KiB of characters are parsed after the first paint behind a loading state.

The body is chosen from the diff size, with no manual toggle. Small diffs use one continuous scroll
body. Large ones (many files, many changed lines, or one file big enough to need windowing) switch
to a single-file focus body driven by the file tree. In the focus body a file above 400 changed lines
is cut into fixed-size native `react-diff-view` hunks and only the visible window is mounted with
`@tanstack/react-virtual`, so chat citation jumps, line quoting and collapse/expand keep working on
real rows. Navigation details are in
[`features/diff/README.md`](../../packages/app-shell/src/features/diff/README.md).
