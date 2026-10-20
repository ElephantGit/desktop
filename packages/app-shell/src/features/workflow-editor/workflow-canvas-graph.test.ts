import { describe, expect, it } from "vitest";
import type { Edge, Node } from "@xyflow/react";
import {
  createMockWorkflowLoopGroup,
  type WorkflowNodeData,
} from "@ora/workflow-mock";
import {
  canvasEdgeInsertPosition,
  insertOnCanvasEdge,
} from "./workflow-canvas-graph";

function node(
  id: string,
  kind: WorkflowNodeData["kind"],
  position = { x: 0, y: 0 },
): Node<WorkflowNodeData, "workflow"> {
  return {
    id,
    type: "workflow",
    position,
    data: { kind, title: id, description: "" },
  };
}

describe("insertOnCanvasEdge", () => {
  it("splits a linear edge through the inserted node at the midpoint", () => {
    const graph = {
      nodes: [
        node("start", "start", { x: 0, y: 80 }),
        node("output", "output", { x: 400, y: 80 }),
      ],
      edges: [
        { id: "e1", source: "start", target: "output", type: "workflow" },
      ] satisfies Edge[],
    };
    const inserted = insertOnCanvasEdge(
      graph,
      "e1",
      [node("agent-1", "agent")],
      "agent-1",
    );

    expect(canvasEdgeInsertPosition(graph, "e1")).toEqual({ x: 200, y: 80 });
    expect(
      inserted.nodes.find((candidate) => candidate.id === "agent-1"),
    ).toMatchObject({
      position: { x: 200, y: 80 },
      selected: true,
    });
    expect(inserted.edges).toEqual([
      { id: "e1", source: "start", target: "agent-1", type: "workflow" },
      {
        id: "edge-1",
        source: "agent-1",
        target: "output",
        type: "workflow",
      },
    ]);
  });

  it("keeps Condition sourceHandle on the incoming half", () => {
    const original: Edge = {
      id: "branch",
      source: "condition",
      sourceHandle: "case-1",
      target: "output",
      targetHandle: "input",
      type: "workflow",
      label: "IF",
    };
    const inserted = insertOnCanvasEdge(
      {
        nodes: [
          node("condition", "condition", { x: 0, y: 0 }),
          node("output", "output", { x: 500, y: 0 }),
        ],
        edges: [original],
      },
      "branch",
      [node("agent-2", "agent")],
      "agent-2",
    );

    expect(inserted.edges).toEqual([
      {
        id: "branch",
        source: "condition",
        sourceHandle: "case-1",
        target: "agent-2",
        type: "workflow",
        label: "IF",
      },
      {
        id: "edge-1",
        source: "agent-2",
        target: "output",
        targetHandle: "input",
        type: "workflow",
      },
    ]);
  });

  it("leaves the graph unchanged when the edge is missing", () => {
    const graph = {
      nodes: [node("start", "start")],
      edges: [] as Edge[],
    };
    expect(
      insertOnCanvasEdge(
        graph,
        "missing",
        [node("agent-1", "agent")],
        "agent-1",
      ),
    ).toBe(graph);
  });

  it("rewires a Loop group through the container node and keeps inner edges", () => {
    const group = createMockWorkflowLoopGroup({
      sequence: 1,
      position: { x: 0, y: 0 },
      locale: "en-US",
    });
    const loop = group.nodes[0]!;
    const inserted = insertOnCanvasEdge(
      {
        nodes: [
          node("start", "start", { x: 0, y: 40 }),
          node("output", "output", { x: 800, y: 40 }),
        ],
        edges: [
          { id: "e1", source: "start", target: "output", type: "workflow" },
        ],
      },
      "e1",
      group.nodes,
      loop.id,
      group.edges,
    );

    expect(inserted.nodes.map((candidate) => candidate.id)).toEqual([
      "start",
      "output",
      loop.id,
      `${loop.id}-start`,
      `${loop.id}-agent`,
    ]);
    expect(
      inserted.edges.some(
        (edge) => edge.source === "start" && edge.target === loop.id,
      ),
    ).toBe(true);
    expect(
      inserted.edges.some(
        (edge) => edge.source === loop.id && edge.target === "output",
      ),
    ).toBe(true);
    expect(inserted.edges).toEqual(expect.arrayContaining(group.edges));
  });
});
