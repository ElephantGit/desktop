import { describe, expect, it } from "vitest";
import type { Edge, Node } from "@xyflow/react";
import { deriveWorkflowVariableCatalog } from "./variable-catalog";
import type { WorkflowNodeData } from "./node-data";

function node(
  id: string,
  data: WorkflowNodeData,
  parentId?: string,
): Node<WorkflowNodeData, "workflow"> {
  return {
    id,
    type: "workflow",
    position: { x: 0, y: 0 },
    data,
    ...(parentId === undefined ? {} : { parentId }),
  };
}

function iterationData(
  iterator: string[],
  collect: string[],
): WorkflowNodeData {
  return {
    kind: "iteration",
    title: "Iterate",
    description: "",
    iterationConfig: {
      iteratorSelector: iterator,
      collectSelector: collect,
      errorStrategy: "fail",
      maxIterations: 10,
    },
  };
}

const START = node("start", {
  kind: "start",
  title: "Start",
  description: "",
  inputVariables: [
    { name: "prs", valueType: "array[object]" },
    { name: "title", valueType: "string" },
  ],
});
const ITER = node("iter", iterationData(["start", "prs"], ["fix", "output"]));
const FIX = node(
  "fix",
  {
    kind: "agent",
    title: "Fix",
    description: "",
    agentConfig: {
      schemaVersion: 3,
      executor: { agentCli: "c", modelId: "m" },
      roleId: "",
      skills: [],
      mcps: [],
      prompt: "fix",
    },
  },
  "iter",
);
const OUT = node("out", { kind: "output", title: "Out", description: "" });

const EDGES: Edge[] = [
  { id: "e1", source: "start", target: "iter" },
  { id: "e2", source: "iter", target: "fix" },
  { id: "e3", source: "fix", target: "out", sourceHandle: null },
  { id: "e4", source: "iter", target: "out" },
];

describe("deriveWorkflowVariableCatalog iteration scope rules", () => {
  it("exposes item and index to region members but never the exposed results", () => {
    const entries = deriveWorkflowVariableCatalog(
      [START, ITER, FIX, OUT],
      EDGES,
      "fix",
    );
    const selectors = entries.map((entry) => entry.selector.join("."));
    expect(selectors).toContain("iter.item");
    expect(selectors).toContain("iter.index");
    // Upstream-of-the-iteration variables stay visible inside the region.
    expect(selectors).toContain("start.prs");
    expect(selectors).toContain("start.title");
    expect(selectors).not.toContain("iter.output");
    expect(selectors).not.toContain("iter.entries");
    expect(selectors).not.toContain("iter.failed_count");
    // The item element type follows the iterator's declared array type.
    const item = entries.find(
      (entry) => entry.selector.join(".") === "iter.item",
    );
    expect(item?.valueType).toBe("object");
  });

  it("exposes the three fixed-type results to outer consumers only", () => {
    const entries = deriveWorkflowVariableCatalog(
      [START, ITER, FIX, OUT],
      EDGES,
      "out",
    );
    const bySelector = new Map(
      entries.map((entry) => [entry.selector.join("."), entry]),
    );
    expect(bySelector.get("iter.output")?.valueType).toBe("array[string]");
    expect(bySelector.get("iter.entries")?.valueType).toBe("array[object]");
    expect(bySelector.get("iter.failed_count")?.valueType).toBe("number");
    // Region members' products stay region-private.
    expect(bySelector.has("fix.output")).toBe(false);
    // Round bindings are region-private.
    expect(bySelector.has("iter.item")).toBe(false);
  });

  it("keeps the exposed types identical across error strategies", () => {
    for (const errorStrategy of ["fail", "continue"] as const) {
      const iter = node(
        "iter",
        iterationData(["start", "prs"], ["fix", "output"]),
      );
      iter.data.iterationConfig = {
        ...iter.data.iterationConfig!,
        errorStrategy,
      };
      const entries = deriveWorkflowVariableCatalog(
        [START, iter, FIX, OUT],
        EDGES,
        "out",
      );
      const bySelector = new Map(
        entries.map((entry) => [entry.selector.join("."), entry]),
      );
      expect(bySelector.get("iter.output")?.valueType).toBe("array[string]");
      expect(bySelector.get("iter.entries")?.valueType).toBe("array[object]");
      expect(bySelector.get("iter.failed_count")?.valueType).toBe("number");
    }
  });

  it("resolves iterator item types from the complete global selector", () => {
    const iter = node(
      "iter",
      iterationData(["vars", "rows"], ["fix", "output"]),
    );
    const entries = deriveWorkflowVariableCatalog(
      [
        node("start", {
          kind: "start",
          title: "Start",
          description: "",
          inputVariables: [{ name: "rows", valueType: "array[number]" }],
        }),
        iter,
        FIX,
        OUT,
      ],
      EDGES,
      "fix",
      [{ name: "vars.rows", valueType: "array[string]" }],
    );
    expect(
      entries.find((entry) => entry.selector.join(".") === "iter.item")
        ?.valueType,
    ).toBe("string");
  });
});

describe("deriveWorkflowVariableCatalog aggregator typing", () => {
  const AGENT = (id: string) =>
    node(id, {
      kind: "agent",
      title: id,
      description: "",
      agentConfig: {
        schemaVersion: 3,
        executor: { agentCli: "c", modelId: "m" },
        roleId: "",
        skills: [],
        mcps: [],
        prompt: "work",
      },
    });

  const aggregatorNode = (variables: string[][]) =>
    node("agg", {
      kind: "aggregator",
      title: "Agg",
      description: "",
      aggregatorConfig: { variables },
    });

  const graph = (agg: ReturnType<typeof aggregatorNode>) => [
    node("start", {
      kind: "start",
      title: "Start",
      description: "",
      inputVariables: [{ name: "limit", valueType: "number" }],
    }),
    AGENT("a"),
    AGENT("b"),
    agg,
    node("out", { kind: "output", title: "Out", description: "" }),
  ];

  const joinEdges: Edge[] = [
    { id: "e1", source: "start", target: "a" },
    { id: "e2", source: "start", target: "b" },
    { id: "e3", source: "a", target: "agg" },
    { id: "e4", source: "b", target: "agg" },
    { id: "e5", source: "agg", target: "out" },
  ];

  it("types the aggregator output as the candidates' common declared type", () => {
    const entries = deriveWorkflowVariableCatalog(
      graph(
        aggregatorNode([
          ["a", "output"],
          ["b", "output"],
        ]),
      ),
      joinEdges,
      "out",
    );
    const output = entries.find(
      (entry) => entry.selector.join(".") === "agg.output",
    );
    expect(output?.valueType).toBe("string");
  });

  it("derives mixed candidate types as any and keeps node outputs string", () => {
    const entries = deriveWorkflowVariableCatalog(
      graph(
        aggregatorNode([
          ["a", "output"],
          ["start", "limit"],
        ]),
      ),
      joinEdges,
      "out",
    );
    const bySelector = new Map(
      entries.map((entry) => [entry.selector.join("."), entry]),
    );
    expect(bySelector.get("agg.output")?.valueType).toBe("any");
  });

  it("derives an empty candidate list as any", () => {
    const entries = deriveWorkflowVariableCatalog(
      graph(aggregatorNode([])),
      joinEdges,
      "out",
    );
    expect(
      entries.find((entry) => entry.selector.join(".") === "agg.output")
        ?.valueType,
    ).toBe("any");
  });

  it("types chained aggregators from their upstream aggregator output", () => {
    const chained = node("agg2", {
      kind: "aggregator",
      title: "Agg2",
      description: "",
      aggregatorConfig: { variables: [["agg", "output"]] },
    });
    const entries = deriveWorkflowVariableCatalog(
      [
        ...graph(
          aggregatorNode([
            ["a", "output"],
            ["b", "output"],
          ]),
        ),
        chained,
        node("out2", { kind: "output", title: "Out2", description: "" }),
      ],
      [
        ...joinEdges,
        { id: "e6", source: "agg", target: "agg2" },
        { id: "e7", source: "agg2", target: "out2" },
      ],
      "out2",
    );
    expect(
      entries.find((entry) => entry.selector.join(".") === "agg2.output")
        ?.valueType,
    ).toBe("string");
  });
});
