# Workflow Run Engine

The execution engine that advances a `workflow_runs` row from `Pending` to a terminal state by
parsing the frozen snapshot graph, scheduling it as a DAG, and driving each node through the
runtime registered for its node type.

## Responsibilities

- **Graph parsing and topology** (`graph.rs`, `node_type.rs`): deserialize a frozen React Flow
  document into a validated `petgraph` DAG, validate structural invariants, and answer topology
  queries (full topological order, successors/predecessors, transitive closures, ready set,
  reachability).
- **Node runtimes and registry** (`node_runtime.rs`, `node_runtime/control.rs`): the per-node-type
  execution strategies behind a registry. Swift runtimes (`Start`, `Condition`, `Output`) complete
  synchronously inside a scheduling wave; the async Agent runtime wraps the backend's
  `NodeExecutor`. The registry is the single place a node type couples to an execution strategy:
  the scheduling core looks runtimes up by node type and dispatches on the registered execution
  form, never on the node type itself.
- **Engine persistence port** (`ports.rs`): the `WorkflowRunEngineRepository` trait that the run
  engine uses, implemented in `ora-db`; plus the `WorkflowRunInvalidationPublisher` port that
  publishes a stateless invalidation after every committed run or node-run state transition.
- **Worktree initializer port** (`ports.rs`): the `WorkflowRunWorktreeInitializer` trait that the
  deploy flow calls to validate roles and resolve Effect-owned skill placements. It returns the
  actual per-node placements as a receipt rather than exposing a directory convention to later
  execution layers.
- **Skill delivery model** (`skill_delivery.rs`): Agent capability, non-empty validated discovery
  roots, frozen materialization bindings, and the typed workflow-run payload shared by deployment
  and node execution.
- **Run engine** (`engine.rs`): `start`/`cancel`/`restart` use cases and the reactive DAG scheduler.
  The scheduling core (`run_schedule`) recomputes state from persistence, hands in-flight nodes to
  their registered runtimes, and finishes drained runs; it contains no node-type branching.

## Non-responsibilities

- Does not persist anything itself; it only defines the persistence port.
- Does not drive Ora sessions; agent execution is delegated through the `NodeExecutor` port that
  the engine wraps as the Agent node runtime.
- Does not resolve roles or materialize skills; role and skill binding validation is wired by the
  backend at deploy time through `WorkflowRunWorktreeInitializer`, while Effect independently owns
  physical Skill materialization. `start` therefore only validates graph executability.
- Does not run the workflow-run CRUD handlers (see the parent `workflow_run` module).
- Start-time graph-structural validation (`validate_executable_graph`, e.g. "output must be
  terminal", unique condition case ids) is graph policy like `WorkflowGraph::parse` itself — it
  inspects node kinds and therefore stays out of the runtime registry's reach.

## Public boundary

Exported from `workflow_run::engine`: `WorkflowRunEngine`, `WorkflowRunControlHandler`,
`NodeExecutor`, `WorkflowRunCallback`, `WorkflowRunEngineRepository`,
`WorkflowRunInvalidationPublisher`, `NoRunInvalidations`, `WorkflowGraph`,
`WorkflowGraphNode`, `AgentConfig`, `AgentExecutor`, `AgentSkill`, `AgentMcp`, `NodeType`,
`GraphError`, `UnknownNodeType`, `AgentSkillDeliveryProvider`, `SkillMaterializationReceipt`,
`WorkflowRunPayload`, and the repository outcome enums including
`BindWorkflowNodeSessionResult`. The node runtime traits and registry are internal to the engine
module; runtimes are registered by the engine's constructors.

## Module interactions

`ora-backend` implements `NodeExecutor` as `WorkflowRunNodeExecutor` and `WorkflowRunCallback` as
`WorkflowRunEngineCallback`, composing both in `build_workflow_run_engine` during `Backend::open`.
The engine wraps the executor as the Agent runtime and registers the swift control runtimes in
one assembly step. The backend also implements `WorkflowRunInvalidationPublisher` as a bridge
onto the application event hub: after every committed state transition the engine publishes one
`AppEvent::WorkflowRunInvalidated { run_id }`, which carries no workflow state — observers
re-query the persisted run. `ora-db` implements `WorkflowRunEngineRepository`. Agent-node
sessions are a live path, not a test-only stub.

## Key invariants

- `WorkflowGraph` is immutable after `parse`; every topology query is deterministic.
- The graph is acyclic (validated by `petgraph::algo::toposort`), has unique node ids, and at most
  one start node; all three are rejected at parse time with a `GraphError` variant.
- The scheduling core is type-agnostic: `run_schedule` and its scheduling-path helpers contain no
  node-type literals, pinned by the `run_schedule_contains_no_node_type_literals` source test.
  Adding a node type means adding a runtime plus one registration line in the registry assembly.
- Swift runtimes complete inside the scheduling wave and receive only in-memory committed facts —
  the `SwiftNodeRuntime` signature gives them no handle through which IO or persistence could be
  performed, so the per-run serial gate is never held across a wait.
- Rust identifiers use `node_type` (aligned with `workflow_node_runs.node_type`); the wire source
  is React Flow's `data.kind`, read through a serde rename.
- Full-graph order and transitive closures use the same topological rank (upstream first), giving
  agent prompt assembly a stable panorama and input lineage.
- Skill discovery roots are validated worktree-relative paths supplied through an Agent capability
  provider. Deployment freezes the actual invocation name and package paths per node; execution
  does not re-resolve those values from the mutable global skill catalog.
- An agent node's `output` is its final assistant text. Complete conversation history belongs to
  the Ora session and is never duplicated into `workflow_node_runs`.
- A running agent node keeps `session_id` absent while its owning prompt is being prepared. The
  backend publishes the binding only after prompt admission, and the repository rejects that
  publication if cancellation or another terminal transition has already won.
- Run invalidation events never carry workflow state: the persisted rows remain the only source
  of truth, and a lost or reordered event only leaves a stale view that the next event or refresh
  clears.

## Failure semantics

`GraphError` distinguishes structural failures: `InvalidJson`, `MissingNodes`, `MissingEdges`,
`InvalidNode`, `UnknownNodeType`, `DanglingEdge`, `CycleDetected`, `MultipleStartNodes`, and
`DuplicateNodeId`. An empty graph is legal; unsupported-but-known node types fail later at
workflow start rather than at parse.

Agent MCP bindings are parsed and validated by `agent_config`; absent bindings default to an empty node allowlist. Runtime availability remains a Session setup responsibility.
