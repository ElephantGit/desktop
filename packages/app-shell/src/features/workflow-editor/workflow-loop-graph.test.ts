import { describe, expect, it } from "vitest";
import {
  createMockWorkflowLoopGroup,
  createMockWorkflowNode,
} from "@ora/workflow-mock";
import { insertLoopMember } from "./workflow-loop-graph";
import { isValidWorkflowConnection } from "./workflow-flow/connection-validation";

describe("loop member insertion", () => {
  it("preserves occupied branches and loop bindings while placing connectable siblings", () => {
    const graph = createMockWorkflowLoopGroup({
      sequence: 1,
      position: { x: 0, y: 0 },
      locale: "en-US",
    });
    const insertion = {
      type: "loop-output" as const,
      loopId: "loop-1",
      sourceId: "loop-1-start",
    };
    const first = insertLoopMember(
      graph,
      insertion,
      createMockWorkflowNode({
        kind: "agent",
        sequence: 1,
        position: { x: 0, y: 0 },
        locale: "en-US",
      }),
    );
    const second = insertLoopMember(
      first,
      insertion,
      createMockWorkflowNode({
        kind: "agent",
        sequence: 2,
        position: { x: 0, y: 0 },
        locale: "en-US",
      }),
    );
    expect(second.nodes[0]!.data).toEqual(graph.nodes[0]!.data);
    expect(second.edges.slice(0, graph.edges.length)).toEqual(graph.edges);
    expect(new Set(second.edges.map((edge) => edge.id)).size).toBe(
      second.edges.length,
    );
    const added = second.nodes.slice(-2);
    expect(
      added.map((node) => ({
        parentId: node.parentId,
        containerId: node.data.containerId,
      })),
    ).toEqual([
      { parentId: "loop-1", containerId: "loop-1" },
      { parentId: "loop-1", containerId: "loop-1" },
    ]);
    expect(added[1]!.position.y).toBeGreaterThan(added[0]!.position.y);
    expect(
      isValidWorkflowConnection({
        nodes: second.nodes,
        edges: second.edges,
        reconnectingEdgeId: null,
        connection: {
          source: "agent-2",
          target: "loop-1-agent",
          sourceHandle: null,
          targetHandle: null,
        },
      }),
    ).toBe(true);
    expect(
      isValidWorkflowConnection({
        nodes: second.nodes,
        edges: second.edges,
        reconnectingEdgeId: null,
        connection: {
          source: "agent-2",
          target: "loop-1",
          sourceHandle: null,
          targetHandle: null,
        },
      }),
    ).toBe(false);
  });
});
