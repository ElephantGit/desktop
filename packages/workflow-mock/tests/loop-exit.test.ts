import { describe, expect, it } from "vitest";
import { createMockWorkflow } from "../src/fixtures";
import {
  createMockWorkflowLoopGroup,
  createMockWorkflowNode,
} from "../src/node-factory";
import { isDemoWorkflow } from "../src/validation";

describe("importing loop exit nodes", () => {
  it("accepts a terminal loop member and rejects detached or outgoing exits", () => {
    const group = createMockWorkflowLoopGroup({
      sequence: 1,
      position: { x: 0, y: 0 },
      locale: "en-US",
    });
    const exit = createMockWorkflowNode({
      kind: "loopExit",
      sequence: 1,
      position: { x: 400, y: 80 },
      locale: "en-US",
    });
    exit.parentId = "loop-1";
    exit.data.containerId = "loop-1";
    const graph = {
      ...createMockWorkflow("en-US"),
      nodes: [...group.nodes, exit],
      edges: [
        ...group.edges,
        {
          id: "exit-edge",
          source: "loop-1-agent",
          target: exit.id,
          type: "workflow",
        },
      ],
    };
    expect(isDemoWorkflow(graph)).toBe(true);
    expect(
      isDemoWorkflow({
        ...graph,
        nodes: [
          ...group.nodes,
          {
            ...exit,
            parentId: undefined,
            data: { ...exit.data, containerId: undefined },
          },
        ],
      }),
    ).toBe(false);
    expect(
      isDemoWorkflow({
        ...graph,
        edges: [
          ...graph.edges,
          {
            id: "invalid",
            source: exit.id,
            target: "loop-1-agent",
            type: "workflow",
          },
        ],
      }),
    ).toBe(false);
  });
});
