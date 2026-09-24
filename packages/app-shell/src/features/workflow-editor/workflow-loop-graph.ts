import type { Edge, Node } from "@xyflow/react";
import {
  WORKFLOW_LOOP_NODE_HEIGHT,
  WORKFLOW_LOOP_NODE_WIDTH,
  WORKFLOW_NODE_INITIAL_HEIGHT,
  WORKFLOW_NODE_WIDTH,
  type WorkflowNodeData,
} from "@ora/workflow-mock";
import type { LoopOutputInsertion } from "./workflow-container-insertion";

type WorkflowNode = Node<WorkflowNodeData, "workflow">;

/** Adds a loop-owned successor without changing existing branches or feedback bindings. */
export function insertLoopMember<
  TGraph extends { nodes: WorkflowNode[]; edges: Edge[] },
>(
  graph: TGraph,
  insertion: LoopOutputInsertion,
  inputNode: WorkflowNode,
): TGraph {
  const loop = graph.nodes.find((node) => node.id === insertion.loopId);
  const source = graph.nodes.find((node) => node.id === insertion.sourceId);
  if (
    loop?.data.kind !== "loop" ||
    source?.data.containerId !== loop.id ||
    source.data.kind === "output"
  ) {
    return graph;
  }
  const widthOf = (node: WorkflowNode) =>
    node.measured?.width ??
    node.width ??
    (node.data.kind === "condition" ? 320 : WORKFLOW_NODE_WIDTH);
  const heightOf = (node: WorkflowNode) =>
    node.measured?.height ?? node.height ?? WORKFLOW_NODE_INITIAL_HEIGHT;
  const position = {
    x: source.position.x + widthOf(source) + 100,
    y: source.position.y,
  };
  const members = graph.nodes.filter(
    (node) => node.data.containerId === loop.id,
  );
  // Move past overlapping siblings, including branches already attached to this port.
  for (;;) {
    const overlap = members.find(
      (node) =>
        position.x < node.position.x + widthOf(node) + 48 &&
        position.x + widthOf(inputNode) + 48 > node.position.x &&
        position.y < node.position.y + heightOf(node) + 52 &&
        position.y + heightOf(inputNode) + 52 > node.position.y,
    );
    if (!overlap) break;
    position.y = overlap.position.y + heightOf(overlap) + 52;
  }
  const node: WorkflowNode = {
    ...inputNode,
    parentId: loop.id,
    extent: "parent",
    position,
    data: { ...inputNode.data, containerId: loop.id },
  };
  const width = Math.max(
    loop.width ??
      loop.measured?.width ??
      loop.initialWidth ??
      WORKFLOW_LOOP_NODE_WIDTH,
    position.x + widthOf(node) + 48,
  );
  const height = Math.max(
    loop.height ??
      loop.measured?.height ??
      loop.initialHeight ??
      WORKFLOW_LOOP_NODE_HEIGHT,
    position.y + heightOf(node) + 40,
  );
  const occupied = new Set(
    [...graph.nodes, ...graph.edges, node].map((item) => item.id),
  );
  let sequence = 1;
  while (occupied.has(`edge-${sequence}`)) sequence += 1;
  return {
    ...graph,
    nodes: [
      ...graph.nodes.map((candidate) =>
        candidate.id === loop.id
          ? {
              ...candidate,
              width,
              height,
              style: { ...candidate.style, width, height },
            }
          : candidate,
      ),
      node,
    ],
    edges: [
      ...graph.edges,
      {
        id: `edge-${sequence}`,
        type: "workflow",
        source: source.id,
        target: node.id,
        ...(insertion.sourceHandle == null
          ? {}
          : { sourceHandle: insertion.sourceHandle }),
      },
    ],
  };
}
