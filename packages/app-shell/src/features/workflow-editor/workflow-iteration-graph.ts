import type { Edge, Node, XYPosition } from "@xyflow/react";
import {
  WORKFLOW_ITERATION_NODE_HEIGHT,
  WORKFLOW_ITERATION_NODE_WIDTH,
  WORKFLOW_NODE_INITIAL_HEIGHT,
  WORKFLOW_NODE_WIDTH,
  type WorkflowNodeData,
} from "@ora/workflow-mock";

export interface IterationGraph {
  nodes: Node<WorkflowNodeData, "workflow">[];
  edges: Edge[];
}

export type IterationInsertion =
  | { type: "entry"; iterationId: string }
  | { type: "edge"; iterationId: string; edgeId: string }
  | {
      type: "output";
      iterationId: string;
      sourceId: string;
      sourceHandle?: string | null;
    };

export interface IterationDeletionRepair<TGraph extends IterationGraph> {
  graph: TGraph;
  clearedCollectSelectorIterationIds: string[];
}

export interface IterationDeletionCascade {
  nodeIds: Set<string>;
  edgeIds: Set<string>;
  memberCount: number;
}

const ITERATION_MEMBER_LEFT = 96;
const ITERATION_MEMBER_TOP = 160;
const ITERATION_MEMBER_COLUMN_GAP = 100;
const ITERATION_MEMBER_ROW_GAP = 52;
const ITERATION_FRAME_RIGHT_PADDING = 48;
const ITERATION_FRAME_BOTTOM_PADDING = 40;
const CONDITION_NODE_WIDTH = 320;

/** Inserts one iteration member and rewires the selected insertion point atomically. */
export function insertIterationMember<TGraph extends IterationGraph>(
  graph: TGraph,
  insertion: IterationInsertion,
  inputNode: Node<WorkflowNodeData, "workflow">,
): TGraph {
  const iteration = graph.nodes.find(
    (node) =>
      node.id === insertion.iterationId && node.data.kind === "iteration",
  );
  if (iteration === undefined) {
    return graph;
  }
  const node = prepareIterationMember(
    inputNode,
    insertion.iterationId,
    insertionPosition(graph, insertion),
  );
  const occupiedIds = [
    ...graph.nodes.map((candidate) => candidate.id),
    ...graph.edges.map((edge) => edge.id),
    node.id,
  ];
  let edges: Edge[];
  switch (insertion.type) {
    case "entry":
      edges = [
        ...graph.edges,
        {
          id: uniqueGraphId("edge", occupiedIds),
          source: insertion.iterationId,
          sourceHandle: "iteration-entry",
          target: node.id,
          type: "workflow",
        },
      ];
      break;
    case "output":
      edges = [
        ...graph.edges,
        {
          id: uniqueGraphId("edge", occupiedIds),
          source: insertion.sourceId,
          ...(insertion.sourceHandle == null
            ? {}
            : { sourceHandle: insertion.sourceHandle }),
          target: node.id,
          type: "workflow",
        },
      ];
      break;
    case "edge": {
      const edge = graph.edges.find(
        (candidate) => candidate.id === insertion.edgeId,
      );
      if (edge === undefined) {
        return graph;
      }
      const { targetHandle, ...sourceEdge } = edge;
      edges = graph.edges.flatMap((candidate) =>
        candidate.id === edge.id
          ? [
              { ...sourceEdge, target: node.id },
              {
                id: uniqueGraphId("edge", occupiedIds),
                source: node.id,
                target: edge.target,
                type: edge.type,
                ...(targetHandle == null ? {} : { targetHandle }),
              },
            ]
          : [candidate],
      );
      break;
    }
  }
  return resizeIterationFrames(
    { ...graph, nodes: [...graph.nodes, node], edges } as TGraph,
    [insertion.iterationId],
    "expand",
  );
}

/** Expands the selected frames to contain every member without shrinking authored space. */
export function expandIterationFrames<TGraph extends IterationGraph>(
  graph: TGraph,
  iterationIds?: readonly string[],
): TGraph {
  return resizeIterationFrames(graph, iterationIds, "expand");
}

/** Recomputes the smallest valid frame around each selected region. */
export function compactIterationFrames<TGraph extends IterationGraph>(
  graph: TGraph,
  iterationIds?: readonly string[],
): TGraph {
  return resizeIterationFrames(graph, iterationIds, "compact");
}

/** Clears iteration collect selectors whose owning member was deleted, then compacts frames. */
export function repairIterationGraphAfterNodeDeletion<
  TGraph extends IterationGraph,
>(
  graph: TGraph,
  removedNodeIds: ReadonlySet<string>,
): IterationDeletionRepair<TGraph> {
  const clearedCollectSelectorIterationIds: string[] = [];
  const nodes = graph.nodes.map((node) => {
    if (node.data.kind !== "iteration") {
      return node;
    }
    const config = node.data.iterationConfig;
    const collectedNodeId = config?.collectSelector[0];
    if (
      config === undefined ||
      collectedNodeId === undefined ||
      !removedNodeIds.has(collectedNodeId)
    ) {
      return node;
    }
    clearedCollectSelectorIterationIds.push(node.id);
    return {
      ...node,
      data: {
        ...node.data,
        iterationConfig: { ...config, collectSelector: [] },
      },
    };
  });
  return {
    graph: compactIterationFrames({ ...graph, nodes } as TGraph),
    clearedCollectSelectorIterationIds,
  };
}

/** Resolves members and incident edges that must join an iteration-container deletion. */
export function resolveIterationDeletionCascade(
  graph: IterationGraph,
  requestedNodeIds: ReadonlySet<string>,
): IterationDeletionCascade {
  const iterationIds = new Set(
    graph.nodes
      .filter(
        (node) =>
          requestedNodeIds.has(node.id) && node.data.kind === "iteration",
      )
      .map((node) => node.id),
  );
  const nodeIds = new Set(requestedNodeIds);
  let memberCount = 0;
  for (const node of graph.nodes) {
    if (node.parentId !== undefined && iterationIds.has(node.parentId)) {
      nodeIds.add(node.id);
      memberCount += 1;
    }
  }
  return {
    nodeIds,
    edgeIds: new Set(
      graph.edges
        .filter((edge) => nodeIds.has(edge.source) || nodeIds.has(edge.target))
        .map((edge) => edge.id),
    ),
    memberCount,
  };
}

/** Returns the persisted expanded size, defaulting old snapshots to the supported minimum. */
export function iterationExpandedSize(
  node: Node<WorkflowNodeData, "workflow">,
): { width: number; height: number } {
  return {
    width: finiteSize(node.initialWidth, WORKFLOW_ITERATION_NODE_WIDTH),
    height: finiteSize(node.initialHeight, WORKFLOW_ITERATION_NODE_HEIGHT),
  };
}

/** Normalizes a new region member without persisting React Flow's presentation-only extent. */
function prepareIterationMember(
  node: Node<WorkflowNodeData, "workflow">,
  iterationId: string,
  position: XYPosition,
): Node<WorkflowNodeData, "workflow"> {
  const serializable = { ...node };
  delete serializable.extent;
  delete serializable.expandParent;
  const data =
    node.data.kind === "agent" && node.data.agentConfig !== undefined
      ? {
          ...node.data,
          agentConfig: { ...node.data.agentConfig, interactive: false },
        }
      : node.data;
  return {
    ...serializable,
    parentId: iterationId,
    position,
    data,
  };
}

/** Places a new member close to the selected graph seam while keeping it inside the frame. */
function insertionPosition(
  graph: IterationGraph,
  insertion: IterationInsertion,
): XYPosition {
  const members = graph.nodes.filter(
    (node) => node.parentId === insertion.iterationId,
  );
  if (insertion.type === "entry") {
    const entryCount = graph.edges.filter(
      (edge) =>
        edge.source === insertion.iterationId &&
        edge.sourceHandle === "iteration-entry",
    ).length;
    return {
      x: ITERATION_MEMBER_LEFT,
      y:
        ITERATION_MEMBER_TOP +
        entryCount * (WORKFLOW_NODE_INITIAL_HEIGHT + ITERATION_MEMBER_ROW_GAP),
    };
  }
  if (insertion.type === "edge") {
    const edge = graph.edges.find(
      (candidate) => candidate.id === insertion.edgeId,
    );
    if (edge !== undefined) {
      const source = graph.nodes.find((node) => node.id === edge.source);
      const target = graph.nodes.find((node) => node.id === edge.target);
      if (source !== undefined && target !== undefined) {
        const sourcePosition =
          source.id === insertion.iterationId
            ? { x: 0, y: ITERATION_MEMBER_TOP }
            : source.position;
        return {
          x: Math.max(
            ITERATION_MEMBER_LEFT,
            Math.round((sourcePosition.x + target.position.x) / 2),
          ),
          y: Math.max(
            ITERATION_MEMBER_TOP,
            Math.round((sourcePosition.y + target.position.y) / 2),
          ),
        };
      }
    }
  }
  if (insertion.type === "output") {
    const source = graph.nodes.find((node) => node.id === insertion.sourceId);
    if (source !== undefined) {
      const siblingOutputs = graph.edges.filter(
        (edge) =>
          edge.source === insertion.sourceId &&
          edge.sourceHandle === insertion.sourceHandle,
      ).length;
      return {
        x: source.position.x + nodeWidth(source) + ITERATION_MEMBER_COLUMN_GAP,
        y:
          source.position.y +
          siblingOutputs *
            (WORKFLOW_NODE_INITIAL_HEIGHT + ITERATION_MEMBER_ROW_GAP),
      };
    }
  }
  return {
    x: ITERATION_MEMBER_LEFT,
    y:
      ITERATION_MEMBER_TOP +
      members.length *
        (WORKFLOW_NODE_INITIAL_HEIGHT + ITERATION_MEMBER_ROW_GAP),
  };
}

/** Applies either monotonic expansion or compact fitting to selected iteration frames. */
function resizeIterationFrames<TGraph extends IterationGraph>(
  graph: TGraph,
  iterationIds: readonly string[] | undefined,
  mode: "expand" | "compact",
): TGraph {
  const selectedIds = iterationIds === undefined ? null : new Set(iterationIds);
  let changed = false;
  const nodes = graph.nodes.map((node) => {
    if (
      node.data.kind !== "iteration" ||
      (selectedIds !== null && !selectedIds.has(node.id))
    ) {
      return node;
    }
    const members = graph.nodes.filter(
      (candidate) => candidate.parentId === node.id,
    );
    const requiredWidth = Math.max(
      WORKFLOW_ITERATION_NODE_WIDTH,
      ...members.map(
        (member) =>
          member.position.x + nodeWidth(member) + ITERATION_FRAME_RIGHT_PADDING,
      ),
    );
    const requiredHeight = Math.max(
      WORKFLOW_ITERATION_NODE_HEIGHT,
      ...members.map(
        (member) =>
          member.position.y +
          nodeHeight(member) +
          ITERATION_FRAME_BOTTOM_PADDING,
      ),
    );
    const current = iterationExpandedSize(node);
    const width = snapSize(
      mode === "expand"
        ? Math.max(current.width, requiredWidth)
        : requiredWidth,
    );
    const height = snapSize(
      mode === "expand"
        ? Math.max(current.height, requiredHeight)
        : requiredHeight,
    );
    if (node.initialWidth === width && node.initialHeight === height) {
      return node;
    }
    changed = true;
    return { ...node, initialWidth: width, initialHeight: height };
  });
  return changed ? ({ ...graph, nodes } as TGraph) : graph;
}

/** Returns the visual width used for fitting and insertion. */
function nodeWidth(node: Node<WorkflowNodeData, "workflow">): number {
  return finiteSize(
    node.measured?.width ?? node.width ?? node.initialWidth,
    node.data.kind === "condition" ? CONDITION_NODE_WIDTH : WORKFLOW_NODE_WIDTH,
  );
}

/** Returns the visual height used for fitting and insertion. */
function nodeHeight(node: Node<WorkflowNodeData, "workflow">): number {
  return finiteSize(
    node.measured?.height ?? node.height ?? node.initialHeight,
    WORKFLOW_NODE_INITIAL_HEIGHT,
  );
}

/** Accepts only positive finite dimensions from persisted or measured geometry. */
function finiteSize(value: number | undefined, fallback: number): number {
  return value !== undefined && Number.isFinite(value) && value > 0
    ? value
    : fallback;
}

/** Aligns frame dimensions with the editor's twenty-pixel grid. */
function snapSize(value: number): number {
  return Math.ceil(value / 20) * 20;
}

/** Produces a stable unused graph id without leaking editor state into the transform. */
function uniqueGraphId(prefix: string, occupiedIds: Iterable<string>): string {
  const occupied = new Set(occupiedIds);
  let sequence = 1;
  while (occupied.has(`${prefix}-${sequence}`)) {
    sequence += 1;
  }
  return `${prefix}-${sequence}`;
}
