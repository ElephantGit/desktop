# Workspace Files

English | [中文](workspace-files.zh.md)

The Files panel browses a checkout, views text, searches by name and content, refreshes on native
file changes, and supports create, cut/copy/paste, rename and confirmed delete from the explorer
tree. The viewer itself is read-only. The same operations back `@` file mentions in the composer.

## Ownership

The client sends a task or project id and, where needed, a checkout-relative path; it never supplies
a root. Desktop resolves the authoritative checkout from persisted identity.

| Layer | Owns |
|---|---|
| `ora-utils` | Portable path validation and canonical containment checks |
| `ora-fs` | Applying them to checkout roots, read and search bounds, ripgrep execution, native watching |
| `apps/desktop/src-tauri/src/commands/files.rs` | Tauri extraction, root resolution, the command/channel boundary |
| `apps/desktop/src-tauri/src/workspace_files.rs` | Mapping filesystem results to contract values with typed errors |
| `packages/app-shell/src/features/files` | Tree, viewer, search UI and line quoting |
| `packages/app-shell/src/state/data/files.ts` | Query identity, invalidation and refresh scope |

General filesystem access deliberately stays out of `ora-backend`.

## Scope: task worktree or project checkout

With a task selected, Files resolves against that task's worktree. With a project selected but no
task yet (a blank or draft composer), Explorer, Search and `@` mentions resolve against the project's
Main Workspace checkout, the same directory used for model discovery and the first session. When
both ids are present the task worktree wins.

## Operations

| Task scope | Project scope | Request |
|---|---|---|
| `listWorkspaceDirectory` | `listProjectDirectory` | Id, optional relative `path` |
| `readWorkspaceFile` | `readProjectFile` | Id, relative `path` |
| `searchWorkspace` | `searchProject` | Id, bounded query |
| `watchWorkspace` | `watchProject` | Id; a stream |
| `createWorkspaceEntry` | `createProjectEntry` | Id, relative `path`, `kind` (file or directory) |
| `copyWorkspaceEntry` | `copyProjectEntry` | Id, `from`, destination `path` |
| `moveWorkspaceEntry` | `moveProjectEntry` | Id, `from`, destination `path` |
| `deleteWorkspaceEntry` | `deleteProjectEntry` | Id, relative `path` |

All returned paths are slash-separated and relative to the resolved checkout. Contracts live in
`crates/contracts/src/file_system.rs`.

- Create refuses to replace an existing path (`file_system_path_already_exists`) and requires the
  parent to exist. The explorer creates inside a folder or in a file's parent; blank tree space
  creates at the root.
- Copy and paste act on the entry itself; a same-folder copy gets a `name copy` suffix. Rename is an
  inline move of the basename. Delete asks for confirmation, then permanently removes the path.
- Watch streams emit `data`, `error` and `end` frames. Error frames use the shared
  `{ code, params, requestId }` shape, and a terminal error queued during shutdown is emitted as
  `error`, not as a successful `end`. Changes arrive as cache-invalidating batches, not an event log.
  Closing a watcher awaits native release, see [Streams and cancellation](../core/streams.md).

## Safety

Paths are validated as relative paths, canonicalized before containment checks and bounded before
reads, searches or creates. Create is a contained `create_new` or `create_dir`; copy, move and
delete refuse to escape the checkout and never follow symbolic links (delete removes an unfollowed
symlink itself). The backend rejects rooted paths.

## Refresh

Successful create, copy, move and delete await the Files data owner's scope refresh for directory,
file and search queries even before a watcher event arrives. Active queries refetch and inactive ones
become stale; other tasks and checkouts stay isolated. Rename events invalidate both paths, their
parents and searches; a rescan invalidates the whole selected scope. Changing scope aborts the old
watcher and unmount aborts the current one.

## Viewer and chat integration

- Gutter `+` (click, drag, or Ctrl/Cmd+Enter on a focused line number) quotes lines into the
  composer as a compact chip. The prompt carries a backtick `path:range` reference and the agent
  reads the body itself; chat history renders it back as the same chip. Diff-gutter quotes are the
  exception and expand to a small `diff --git` patch, because the change is not on disk yet.
- Inline artifact links in chat open the panel through `openWorkspaceFile` with a
  `WorkspaceFileRequest` (`path`, `requestId`, optional line, column and end line). The view strips
  the task-cwd prefix from absolute ACP paths, expands ancestors and highlights the cited line or
  range. A new `requestId` invalidates that path's query, so reopening re-reads the disk. Paths
  outside the task cwd are not opened, and a missing file shows localized missing-path copy instead
  of a transport error.
- Files up to 400 lines mount every row. Larger files render only the window around the viewport;
  above about 512 KiB of characters, content passes are deferred past the first paint behind a
  loading overlay so switching sessions never waits on the preview. Syntax highlighting is capped at
  512 KiB.
- Search and reads are cancellable through the contracts client.

Copy Path, Copy Relative Path and Reveal in File Manager use the native `open_location` command,
see [Desktop host](../desktop/desktop-host.md#native-commands).
