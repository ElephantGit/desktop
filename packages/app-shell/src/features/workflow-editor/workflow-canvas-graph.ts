import type { Edge, Node, XYPosition } from "@xyflow/react";
import type { WorkflowNodeData } from "@ora/workflow-mock";

export interface CanvasGraph {
  nodes: Node<WorkflowNodeData, "workflow">[];
  edges: Edge[];
}

/** Places a canvas insert on the selected edge's midpoint in authored coordinates. */
export function canvasEdgeInsertPosition(
  graph: CanvasGraph,
  edgeId: string,
): XYPosition | null {
  const edge = graph.edges.find((candidate) => candidate.id === edgeId);
  if (edge === undefined) {
    return null;
  }
  const source = graph.nodes.find((node) => node.id === edge.source);
  const target = graph.nodes.find((node) => node.id === edge.target);
  if (source === undefined || target === undefined) {
    return null;
  }
  return {
    x: Math.round((source.position.x + target.position.x) / 2),
    y: Math.round((source.position.y + target.position.y) / 2),
  };
}

/**
 * Inserts one or more canvas nodes on an existing edge and rewires that edge
 * through `throughNodeId` in one graph replacement.
 *
 * Incoming handle metadata stays on the original edge; the outgoing half is a
 * new edge so Condition branches and target ports keep their authored meaning.
 */
export function insertOnCanvasEdge<TGraph extends CanvasGraph>(
  graph: TGraph,
  edgeId: string,
  nodesToAdd: Node<WorkflowNodeData, "workflow">[],
  throughNodeId: string,
  extraEdges: Edge[] = [],
): TGraph {
  const edge = graph.edges.find((candidate) => candidate.id === edgeId);
  const through = nodesToAdd.find((node) => node.id === throughNodeId);
  const position = canvasEdgeInsertPosition(graph, edgeId);
  if (edge === undefined || through === undefined || position === null) {
    return graph;
  }
  const occupiedIds = [
    ...graph.nodes.map((node) => node.id),
    ...graph.edges.map((candidate) => candidate.id),
    ...nodesToAdd.map((node) => node.id),
    ...extraEdges.map((candidate) => candidate.id),
  ];
  const placed = nodesToAdd.map((node) =>
    node.id === throughNodeId
      ? { ...node, position, selected: true }
      : { ...node, selected: false },
  );
  const { targetHandle, ...sourceEdge } = edge;
  const outgoingId = uniqueGraphId("edge", occupiedIds);
  const edges = [
    ...graph.edges.flatMap((candidate) =>
      candidate.id === edge.id
        ? [
            { ...sourceEdge, target: throughNodeId },
            {
              id: outgoingId,
              source: throughNodeId,
              target: edge.target,
              type: edge.type,
              ...(targetHandle == null ? {} : { targetHandle }),
            },
          ]
        : [candidate],
    ),
    ...extraEdges,
  ];
  return {
    ...graph,
    nodes: [
      ...graph.nodes.map((node) => ({ ...node, selected: false })),
      ...placed,
    ],
    edges,
  } as TGraph;
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
