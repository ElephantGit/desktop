# Changing the Code

English | [中文](changing-the-code.zh.md)

Start from the owning use case, not from the number of files. DTOs, generated outputs, tests and
documents are separate categories; many changed files do not by themselves mean duplicated facts. The
principles behind these steps (single source of truth, domain ownership, minimal interfaces, lifecycle
and isolation, behavioral verification) are in `AGENTS.md`.

## Adding an operation to an existing domain

The four hand-written facts are the **contract**, the **logical operation**, the **Desktop binding** and
the **business implementation**. Everything else is generated or composed from them.

1. **Contract.** Define the request and response DTOs in the owning `crates/contracts/src/<domain>.rs`
   and register their TypeScript exports in that module's `export` function. DTO module filenames are
   kebab-case (`#[ts(export_to = "workflow-run.ts")]`). Keep transport routing and Webview
   authorization out of DTOs. A genuinely new DTO family also needs its one explicit module and export
   composition entry.
2. **Logical operation.** Add it under `xtask/src/frontend/namespaces/<namespace>.rs` with its operation
   name, client namespace and member, DTO names and explicit `FrontendResponseMode::Unary` or `Stream`.
3. **Desktop binding.** Add it under `apps/desktop/src-tauri/bindings/`. Unary bindings name their Rust
   handler and `Permission`; stream bindings name a domain startup handler and reuse the authorized
   `stream_contract`/`cancel_contract_stream` pair. Native commands without an SDK operation also belong
   to this catalog.
4. **Implementation.** Implement the use case in its domain module. Backend operations use a narrow
   domain handle: do not add a root `Backend` forwarding method or expose storage, locks or supervisors.
   The Desktop adapter invokes the handle through the shared request lifecycle helpers; a file stream's
   startup belongs to `commands/files.rs`, and `StreamStart` owns registration, cancellation, startup
   settlement and forwarding.
5. **Generate.** Run `task export-contracts`, then `task check:contracts`. Never hand-edit client
   forwarding, DTO barrels, transport maps, stream dispatch, command registration or permissions. Repeat
   generation to verify determinism; checks never repair drift or delete unknown hand-written files.
6. **Test** at the operation's real interface: backend tests keep real SQLite and Git where those are
   the behavior under test, and frontend tests use the production client with typed handlers. Verify DTO
   and option propagation, errors with request ids and lifecycle outcomes; streams need consumption,
   abort or unmount, and cleanup evidence.
7. **Verify** with the smallest relevant tasks, then the package checks. Desktop changes need
   `task test:tauri`, cross-cutting changes `task test`. See [Testing](testing.md).

## Frontend ownership checklist

- Translations stay in the owning feature; add a resource to the root composition only for a new owner.
  Key parity and duplicate checks must pass.
- Shared Cut, Copy, Paste and Select All copy belongs to `features/editor/translations.ts`; Chat-specific
  copy stays with Chat.
- Query identity, authoritative response adoption and invalidation belong to the data owner under
  `state/data/`. Keep meaningful key distinctions, deletion cascades and event refresh scopes; the UI must
  not copy query-key strings.
- Declare only the public symbols other features need in `interface.json`, with their purpose. Shared
  state must not depend on feature UI.
- Compose test memory adapters explicitly; a missing operation must fail.
- Decide who owns each watcher, event bridge, timer, observer and native surface, and dispose them on
  scope change, unmount and shutdown.

See [Frontend architecture](../core/frontend-architecture.md).

## Removing a feature

1. Identify its public consumers and durable data first. Removing a feature never authorizes deleting
   user files, and the local filesystem layout is a hard compatibility boundary: if an old path could
   collide with a new layout or block safe startup, design detection, migration or isolation first.
2. Remove its implementation, DTOs and export entries, logical catalog entries, Desktop bindings,
   obsolete composition, interface declarations and translations. Do not keep root forwarding or a second
   implementation as a compatibility layer.
3. Update consumers through their domain interfaces. For shared data, check whether other owners still
   need the query factory and invalidation policy. Stop subscriptions and remove feature-owned cached and
   draft state without erasing other Workspaces' state.
4. Regenerate. Removed operations must disappear from generated clients, routing, DTO exports,
   registration and permissions; check tracked diffs and untracked generated files, and confirm the main
   and plugin Webview capabilities separately.
5. Move tests with surviving responsibilities, and drop a test obligation only when the behavior is
   intentionally removed or directly covered elsewhere.
6. Search for old operation names, imports, query keys, translation keys and fixtures. Run generation,
   feature-interface and Rust-size checks, remove stale size exceptions, and update the affected
   documents.

Review the final diff by category: implementation and declarations, necessary composition, generated
outputs, tests and documents.

## Documentation

When a change adds or changes behavior, update the matching document under `docs/` in the same change,
both language versions, following the [maintenance rules](../README.md#maintenance-rules). `specs/` is an
independent repository; changing an ADR there follows its own instructions.
