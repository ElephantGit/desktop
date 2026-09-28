import { describe, expect, it } from "vitest";
import {
  createMockWorkflowCapabilities,
  supportsWorkflowNodeScope,
} from "../src/capabilities";

describe("workflow node scopes", () => {
  it("allows only Agent and Condition inside an iteration region", () => {
    const capabilities = createMockWorkflowCapabilities("en-US");

    expect(
      capabilities.nodeTypes
        .filter((nodeType) => supportsWorkflowNodeScope(nodeType, "iteration"))
        .map((nodeType) => nodeType.kind),
    ).toEqual(["agent", "condition"]);
  });

  it("keeps exit nodes inside loops while preserving the outer catalog", () => {
    const capabilities = createMockWorkflowCapabilities("en-US");

    expect(
      capabilities.nodeTypes
        .filter((nodeType) => supportsWorkflowNodeScope(nodeType, "workflow"))
        .map((nodeType) => nodeType.kind),
    ).toEqual(["start", "agent", "condition", "loop", "iteration", "output"]);
    expect(
      capabilities.nodeTypes
        .filter((nodeType) => supportsWorkflowNodeScope(nodeType, "loop"))
        .map((nodeType) => nodeType.kind),
    ).toEqual(["agent", "condition", "output", "loopExit"]);
  });
});
