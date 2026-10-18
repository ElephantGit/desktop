# Workflow Editor Load Path

English | [中文](workflow-editor-load-path-fix.zh.md)

Opening a workflow could leave the entire desktop window blank. This document records what failed,
why one bad node took the whole page down, and how the load path was hardened.

## Symptom

Clicking a workflow in the editor produced a permanently white window: no canvas, no chrome, no
recovery. The renderer reported:

```
Uncaught TypeError: Cannot read properties of undefined (reading 'label')
    at WorkflowFlowNodeView (node.tsx)
```

The backend was healthy throughout — draft, version, and publish requests completed normally and
the process stayed alive. The failure was entirely in the frontend render pass.

## Root cause

The stored graph contained a node whose `data.kind` was `aggregator`, a kind no Ora version
defined at the time. Every step that could have stopped it was, by design, permissive:

1. **Storage is opaque.** The `graph` column holds the React Flow document as a string, and the
   workflow definition handlers pass it through untouched. A workflow package's file text becomes
   the stored graph verbatim, so unknown content reaching the editor is by design, not accident.
2. **The codec is deliberately lenient.** `parseWorkflowGraph`
   (`packages/workflow-runtime/src/graph-codec.ts`) normalizes the viewport and filters
   annotations and global variables, but says nothing about node kinds — its contract is
   round-trip fidelity for fields this version does not understand.
3. **The render catalog is a closed set.** The canvas registers one component per kind, and
   `createMockWorkflowNodeType` (`packages/workflow-mock/src/capabilities.ts`) resolves a kind
   through an exhaustive `switch` with no `default` arm. A kind outside the union returns
   `undefined`.
4. **The node view dereferenced it immediately.** The node component reads
   `createMockWorkflowNodeType(data.kind, locale).label` on its first render.
5. **Nothing caught the error.** React unmounts the tree when a render throws, and neither
   `packages/app-shell` nor `packages/ui` contains an error boundary, so one node view failing
   blanked the whole window instead of the canvas.

The result: content semantically invalid for this version reached the render layer, and the render
layer had no way to express "I do not know this node".

`aggregator` has since been added to the catalog as a first-class kind, so that particular graph
now renders. The class of failure is unchanged — any kind outside the catalog still reproduces it.

## The fix

The rule chosen: **sanitize at the parse boundary every load path shares, and tell the user what
was skipped**, rather than making every render-layer consumer defensive about kinds it cannot draw.

### Runtime (`packages/workflow-runtime`)

- `types.ts` gains `WORKFLOW_NODE_KINDS`, the canonical kind list, and `WorkflowNodeKind` is now
  derived from it (`(typeof WORKFLOW_NODE_KINDS)[number]`), so the list the sanitizer uses and the
  type the compiler checks cannot drift apart.
- `graph-codec.ts` gains `parseWorkflowGraphWithReport(graph)`, returning
  `{ envelope, droppedNodeCount, droppedNodeKinds }`. It drops:
  - nodes that are not usable records (missing a non-empty string `id`, or `data` that is not an
    object), and
  - nodes whose `data.kind` falls outside `WORKFLOW_NODE_KINDS`, and
  - the edges that referenced any dropped node, since an edge into a node that is no longer there
    cannot render.
- `parseWorkflowGraph` is now a thin wrapper over it, so all six load sites — draft hydration,
  version preview, the export preview, the export handler, and the run projection — are protected
  by one rule without each having to opt in.
- Legacy `prompt`/`model` nodes are upgraded to `agent` _before_ the kind check, so a persisted
  legacy node lands on its supported replacement instead of being read as unknown.
- Everything else about the codec is unchanged: invalid JSON still loads as an empty canvas,
  unknown fields still survive a resave, and edges pointing at ids that were never in the envelope
  are still preserved.

### Editor (`packages/app-shell`)

- Draft hydration parses through the reporting variant. Because the hydrate runs during render,
  the report is carried out through a ref and the notice is raised from an effect once that draft
  has committed, so a discarded render cannot produce a toast.
- The version preview reports on the spot, since it is already an async handler.
- The user sees `已跳过 1 个本版本无法渲染的节点（router）` /
  `Skipped 1 nodes this version cannot render (router)` — count and offending kinds, so the loss is
  explained instead of silent. A node dropped for being malformed has no kind to name and reports
  as an unknown kind.
- The run view and the export paths go through the same codec and are protected without a notice,
  since they are read-only projections.

## Verification

- **Unit (`packages/workflow-runtime/src/graph-codec.test.ts`)** — six new cases: every kind in the
  render catalog survives, an unknown kind is dropped and reported, edges into a dropped node go
  while edges between rendered nodes stay, malformed records are dropped without failing the parse,
  legacy kinds upgrade before the check, and an unparseable graph reports nothing dropped. The
  first case spells the expected kinds out rather than deriving them from `WORKFLOW_NODE_KINDS`, so
  shrinking the list fails the test instead of shrinking it; verified by removing three kinds and
  watching it fail.
- **Behavior (`packages/app-shell/src/features/workflow-editor/workflow-editor.test.tsx`)** — a
  draft carrying a `router` node loads the canvas, does not render that node, and raises the
  notice. Re-running with the filter disabled crashes the vitest worker, confirming the test fails
  on the unfixed code.

## Consequences and trade-offs

- **A dropped node leaves the draft on the next save.** Autosave writes the normalized graph, so a
  node this version cannot render is not carried forward. The published snapshot keeps the original
  bytes, so a rollback or a re-import restores it. This is the deliberate consequence of "store
  verbatim, normalize on save": the alternative — preserving nodes nothing can render — would push
  the problem into every consumer of the graph.
- **This is not an error boundary.** The load path is fixed, so an unknown node kind can no longer
  blank the page, but an unforeseen render error elsewhere still would. Adding a boundary remains
  an open follow-up.
- **Unknown kinds are reported, not interpreted.** A foreign kind is not mapped onto a supported
  one automatically. Where a package's intent is clear, the package's document should be corrected
  at the source.
- **The kind list is a contract with the render catalog.** `createMockWorkflowNodeType` has an
  explicit return type and an exhaustive `switch`, so TypeScript already rejects adding a kind to
  the union without a case. What the compiler cannot see is data carrying a kind outside the union,
  which is what this boundary handles.

## Related

- [Workflow](workflow.md) — graph storage and lifecycle.
- [Workflow Plugin (orax) Import](workflow-orax-import.md) — the packaging convention that ships
  graph JSON into the library.
