import { describe, expect, it } from "vitest";
import {
  createMockWorkflowCapabilities,
  supportsWorkflowNodeScope,
} from "../src/capabilities";

describe("workflow node scopes", () => {
  it("allows only Agent, Condition, and Aggregator inside an iteration region", () => {
    const capabilities = createMockWorkflowCapabilities("en-US");

    expect(
      capabilities.nodeTypes
        .filter((nodeType) => supportsWorkflowNodeScope(nodeType, "iteration"))
        .map((nodeType) => nodeType.kind),
    ).toEqual(["agent", "condition", "aggregator"]);
  });

  it("keeps every currently exposed node available in the outer workflow", () => {
    const capabilities = createMockWorkflowCapabilities("en-US");

    expect(
      capabilities.nodeTypes.every((nodeType) =>
        supportsWorkflowNodeScope(nodeType, "workflow"),
      ),
    ).toBe(true);
  });
});
