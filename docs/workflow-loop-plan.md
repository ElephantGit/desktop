# Workflow Loop Implementation Plan

English | [中文](workflow-loop-plan.zh.md)

Status: implementation in progress (P1); Loop execution remains disabled until durable scheduling is implemented. Updated: 2026-09-16.

Progress: the container decoder, typed configuration, explicit binding visibility checks, and parser regression tests are implemented. Formatting passes. Rust test execution is blocked on the local Windows host by the missing MSVC `link.exe`; installing the C++ Build Tools has not yet produced an available toolchain. P1 is not marked complete, and storage, scheduling, contracts delivery, and editor integration remain pending.

Round computation is also implemented as a pure operation: initial carried values, simultaneous typed feedback, termination before limit failure, and named exports. Fresh round pools import only globals and upstream outer values, retain inherited writer ownership, and omit previous child outputs. Regression tests cover swaps, limits, missing/type-invalid values, successful exit, and pool isolation. These operations are not yet connected to durable execution; their tests remain blocked by the same missing linker.

## Goal and design baseline

Support a bounded feedback workflow such as generate → review → revise → review inside Desktop. A Loop owns an executable child graph, typed carried variables, a termination condition, and final outputs. The outer graph and each single-round child graph remain DAGs.

This plan adopts the container-loop product concept discussed for Dify. It defines Ora semantics independently; it does not claim compatibility with Dify DSL, dependency versions, persistence, or current Human Input support. Before implementation, any upstream behavior used as a compatibility requirement must be checked against a pinned revision.

Follow the [feature change guide](feature-change-guide.md), [workflow ownership](workflow.md), and [migration rules](database-migrations.md). This change is planned across existing owners; it does not introduce a second workflow engine.

## Current implementation constraints

- `WorkflowGraph::parse` in `crates/application/src/workflow_run/engine/graph.rs` rejects cycles. `node_type.rs` currently executes Start, Agent, Condition, and Output.
- `engine/branch_projection.rs` projects state by node ID. `engine/variable_pool.rs` and persisted Condition decisions currently use run-level node selectors. Repeating a definition node requires an execution scope in all three owners.
- `engine/engine.rs` completes the whole run when scheduling drains and checks repeated successful Output nodes across run history. Child completion must gain a separate boundary.
- `crates/db/src/repository/workflow_run_engine.rs` owns transactional completion, cancellation, and restart. `crates/backend/src/workflow/run/` owns sessions, interactive completion, prerequisite preparation, and boot recovery.
- `packages/workflow-runtime/src/types.ts` already lists `loop` as a frontend kind. This is not evidence of production execution support; audit its codec, editor catalog, fixtures, and validation before extending it.
- The real run view uses contracts queries and polling; some artifacts/HITL facilities still use the memory runtime. Loop execution and history must use the real contracts path and explicit test adapters.

## Proposed first-delivery semantics

These are implementation defaults to review with the plan, not existing behavior.

| Concern         | Proposed behavior                                                                                                                                                      |
| --------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Shape           | One level of Loop containers; sequential rounds; ordinary branching and joins inside a round                                                                           |
| Entry           | Explicit container ownership and one synthetic child entry; execute at least one round                                                                                 |
| State           | Typed initial carried variables, initialized from constants or visible upstream selectors                                                                              |
| Feedback        | Explicit mapping from this round's values to the next round's carried variables                                                                                        |
| Termination     | Evaluate a typed `until` condition after the active child graph drains; reuse Condition evaluation                                                                     |
| Limit           | Required positive maximum round count; proposed default 5 and ceiling 100, validated on both sides                                                                     |
| Ordering        | Resolve feedback values, evaluate termination against the completed round, then atomically exit or advance; all feedback assignments read the same pre-assignment pool |
| Limit reached   | If termination is still false, fail with a typed limit error and preserve the last round for inspection                                                                |
| Outputs         | Export explicitly named Loop outputs on success; external nodes see these outputs only after Loop completion                                                           |
| Agent execution | A new NodeRun and Session per node per round; preserve existing model, Skill, MCP, and output-contract behavior                                                        |
| Human feedback  | Support existing interactive Agent nodes inside the Loop, including waiting, follow-up turns, and manual completion                                                    |
| Workspace       | Reuse the run's selected Workspace; file edits accumulate across rounds                                                                                                |

The first delivery excludes arbitrary back edges, nested Loops, array Iteration, automatic retry, same-session reuse across rounds, and a separate mid-round Break node. The model must reject unsupported forms explicitly. A reviewer can represent approval through structured output consumed by the Loop's `until` condition.

Example: initialize `draft = ""` and `feedback = ""`; a writer reads the requirement and these carried values; a reviewer produces `approved` and `feedback`; feedback mapping carries the new draft and review text; `approved == true` exports the draft, otherwise the next round begins.

## Graph and variable ownership

Use one canonical container relationship in the serialized graph; derive React Flow parent layout and runtime child membership from that declaration. Do not maintain separate handwritten child lists in multiple layers. Version the new graph shape or provide a deterministic decoder rule; old flat snapshots decode into the root scope without rewriting immutable snapshots.

Validate unique IDs, ownership, child entry, reachability, acyclicity per scope, no cross-boundary edges, legal terminal bindings, limits, and types. External dependencies enter through visible container inputs; outputs leave through the Loop. Parent/child ownership cycles and malformed imports fail before sessions or Skill materialization begin.

Each round has a fresh variable value set and private Condition decisions. Read-only outer inputs and globals are visible according to the existing scope policy. Carried variables have explicit initial values, so round one never reads an unassigned previous output. A round's nodes cannot write another round's pool or mutate outer values implicitly. Type checking, templates, structured selectors, and workspace-file validation continue to use their existing owners.

At round completion, resolve all configured feedback and output selectors with explicit missing-value errors. Inactive branch values cannot be read accidentally; users must bind values available on the selected path. Each output name is unique within its container boundary. Child graph completion uses a container result boundary rather than completing the top-level Run through an Output node.

## Durable execution model

### Graph format being implemented

Container snapshots use `schemaVersion: 2`. A Loop is a root node with `data.kind: "loop"` and `data.loopConfig`. Each child declares `data.containerId` referencing that Loop; an optional renderer `parentId` must match. Both the root graph and each body require one Start node and full reachability. Nested containers and cross-scope edges are rejected. Existing flat snapshots retain their original decoder behavior.

`loopConfig` contains `maxIterations`, `variables`, `until`, and `outputs`. Each variable has `name`, `valueType`, `initial`, and `feedback`. Initial values are tagged as `{ "kind": "constant", "value": ... }` or `{ "kind": "variable", "selector": ["node", "variable"] }`. Feedback is a selector array. `until` uses the existing Condition `logic` and `conditions` syntax. Outputs use `{ "name": "result", "variableSelector": ["child", "output"] }`.

Initializers can reference globals and upstream outer nodes. Feedback, termination, and exports can reference the completed body, carried values, globals, and upstream outer values. Outer nodes must read the Loop's exported results rather than child nodes. This parser boundary does not replace runtime checks for declared variables, value types, missing values, or inactive branches; those checks must be completed before execution is enabled.

### Persistence work remaining

Introduce domain-owned execution scopes and Loop progress; final Rust names are chosen during implementation. A root scope represents today's flat execution. A round scope records its parent Loop NodeRun and round index. NodeRuns reference their scope while retaining their unique execution IDs and definition node IDs.

- Persist scope identity, parent ownership, round index, lifecycle, current node instances, variable values, and private branch decisions. Reuse typed pool representations, but give each execution fact one authoritative storage location.
- Enforce uniqueness for `(scope_id, node_id)` and `(loop_node_run_id, round_index)`. Scope parentage and node-session bindings must belong to the same Run. A restart creates a fresh root execution generation; old callbacks cannot target it.
- Represent Loop progress with enums and associated data, such as executing a child scope or terminal with a reason. Avoid a collection of loosely related optional state fields.
- Commit final child completion, feedback values, termination decision, and next scope creation or parent completion atomically. A resumed scheduler can dispatch an already-created scope without creating it twice.
- Retain historical NodeRuns, inputs, outputs, and Sessions for every round. Pending nodes that have never run remain projections; genuine interactive waiting is distinguished by the bound execution instance.
- Preserve the per-Run serial gate, transaction guards, and execution-ID checks for commands and callbacks. A duplicate or late callback must not release successors or advance a Loop twice.

Add a new ordered Rust migration under `crates/db/src/migration`, including `up` and `down`; never rewrite shipped migration definitions. Backfill old runs and NodeRuns into root scopes while preserving status, payloads, timestamps, and sessions. Test the repository's actual automatic downgrade behavior: an older executable must never resume a flattened active Loop. Specify safe terminalization or archival of unsupported loop execution data before accepting the down migration, and prove that re-upgrade is safe.

No new Workspace or Skill directory layout is required. Baseline side files remain keyed by unique NodeRun IDs. Any additional side-file layout discovered during implementation needs explicit collision handling, migration/isolation, and shared `ora-utils::path` usage.

## Scheduling, sessions, and recovery

Extract scope scheduling and Loop transitions into focused private modules rather than extending the already large engine and executor files. Move relevant tests and invariants with extracted code. Workflow-specific scheduling remains in `ora-application`; only genuinely domain-independent helpers belong in `ora-utils`.

The scheduler dispatches a Loop as a composite NodeRun, schedules the active child scope with the existing DAG rules, and completes the parent only after successful termination. Waiting children keep the parent active. Run completion considers active scopes and composite nodes, so a drained child never finishes the entire Run. Scope-aware Output handling preserves existing flat-workflow behavior.

Deployment traverses child graphs for Agent prerequisites and freezes their Skill receipts. Each round consumes those receipts, the frozen graph, and that round's committed variables. Node prompts include useful round context. Interactive actions target NodeRun IDs, and the existing completion claim prevents racing follow-up turns against manual completion.

Cancellation commits terminal states for the entire owned scope tree before best-effort Session cleanup. A child failure fails its Loop and Run and explicitly settles/stops active siblings; no further round starts. Restart preserves frozen deployment inputs while creating fresh execution state. Delete protections cover composite nodes, waiting children, and live sessions, and never remove the shared Workspace or user files.

Boot reconciliation resumes only durable scheduling gaps, preserves valid interactive waits, and fails interrupted Agent execution under the current policy. It does not automatically replay tools with unknown side effects. Test crashes before/after next-scope creation and before/after dispatch. Missing file baselines continue to degrade change reporting according to the current owner policy. Parallel agents sharing a Workspace provide time-window diffs, not exclusive attribution of edits.

## Contracts and Desktop surfaces

Extend existing workflow/run DTOs with scope, round, parent execution, terminal reason, and history information needed by actual consumers. Keep project lists compact; fetch round details on demand if history becomes large. Prefer extending existing get/list operations; add a dedicated round query only when needed by the history view.

For each new operation, declare DTOs in `crates/contracts`, logical operations in `xtask/src/frontend/namespaces`, Desktop bindings and permissions in `apps/desktop/src-tauri/bindings`, and behavior in the workflow domain handle. Reuse request/stream lifecycle helpers. Generate wiring with `task export-contracts` and verify with `task check:contracts`; do not hand-edit generated TypeScript DTOs, forwarding code, dispatch, or permissions.

In the editor, add the Loop container, child entry, typed initial variables, feedback mappings, termination condition, limit, and outputs. Cover drag/reparent validation, selection, deletion, copy/paste with ID/selector remapping, undo/redo, autosave, version preview, duplication, and import/export. Preserve annotations and canvas data through codecs. Backend validation remains authoritative; shared fixtures exercise agreement with frontend diagnostics.

In Theater/Overview, show Loop progress and termination reason, allow selecting a round and its NodeRuns/Sessions, and preserve that selection through refresh and view switching. Keep current-round projection separate from full history, so identical node IDs from different rounds do not overwrite each other. State/data owns query keys and invalidation; feature owners own translations. Round changes and unmount release owned observers, timers, and subscriptions.

## Implementation sequence and exit criteria

- [ ] **P1 — Contracts and graph design:** settle schema, explicit first-release policies, validation fixtures, scoped variable visibility, and downgrade strategy. Exit: examples for one-round success, feedback, and human review have unambiguous inputs and outputs.
- [ ] **P2 — Storage and execution identity:** add domain scope/progress types, migration, repository ports, unique constraints, and transactional transitions. Exit: old-data upgrade, duplicate advancement, rollback, and downgrade/re-upgrade integration tests pass.
- [ ] **P3 — Engine and backend:** implement scoped DAG scheduling and Loop control; thread scope through prerequisites, prompts, Sessions, interactive completion, cancellation, restart, deletion, and recovery. Exit: production backend interfaces execute multi-round flows using real SQLite and controlled Agent adapters.
- [ ] **P4 — Contracts delivery and frontend:** regenerate SDK/bindings, implement editor/codec support and real round history/projection. Exit: a saved and published Loop can be deployed and observed through Desktop interfaces without depending on mock execution.
- [ ] **P5 — End-to-end verification and documentation:** cover the matrix below, finish full checks, and update both workflow language versions plus owner READMEs. Exit: acceptance evidence is recorded and every unchecked scope item is either delivered or explicitly moved to a follow-up.

## Verification matrix

| Boundary               | Required behavior evidence                                                                                                                                                 |
| ---------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Parser and variables   | Old flat graphs, invalid ownership/cycles, cross-scope selectors, required initial values, feedback type errors, inactive branches, simultaneous feedback assignment       |
| Scheduler              | First-round success, several feedback rounds, exact limit, early termination, branching/join, multiple independent Loops, no premature Run/Output completion               |
| Repository             | Migration up/down/re-upgrade, historical preservation, transaction rollback, duplicate callbacks, stale-generation callbacks, concurrent cancel/advance                    |
| Backend                | Per-round Sessions, frozen Skill/MCP selection, structured-output failure with raw text, manual completion/follow-up race, sibling cleanup, waiting-node delete protection |
| Recovery and isolation | Scheduling-gap recovery, interrupted execution, valid waits, no duplicate round/session dispatch, cross-Run isolation, baseline cleanup, preserved Workspace files         |
| Frontend and Desktop   | Editor round-trip/history, real typed handlers, round selection/cache isolation, request cancellation, permissions, active/finished/waiting presentation                   |

Use the smallest relevant tasks during implementation and inspect `task --list` for authoritative commands. Run `task format`, `task export-contracts`, `task check:contracts`, frontend/Rust lint and tests, and `task test:tauri` for Desktop changes; finish this cross-layer feature with `task test`. Generated-artifact checks supplement behavioral tests.

Rust tests use `pretty_assertions::assert_eq`, injected dependencies, and scoped TRACE logging where callsites are shared. Frontend rendering tests import `appI18n`, await interactions and actual async boundaries, and wrap external-store writes in `act`; clean-stderr warnings must remain failures.

If ADRs or core test cases need changes in `specs/`, first read `specs/AGENTS.md` and inspect status/history with `git -C specs`; deliver those changes as work in its independent repository. This plan itself lives alongside the workflow documentation in `docs/`.
