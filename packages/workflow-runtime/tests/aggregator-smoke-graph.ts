import { serializeWorkflowGraph } from "../src/graph-codec.ts";
import type {
  WorkflowDefinitionEdge,
  WorkflowDefinitionNode,
  WorkflowGlobalVariable,
} from "../src/types.ts";

type SmokeCase = "a" | "b" | "false";

/** Builds one production-codec graph for the backend persistence smoke test. */
export function serializeAggregatorSmokeGraph(smokeCase: SmokeCase): string {
  const agent = (id: "a" | "b"): WorkflowDefinitionNode => ({
    id,
    type: "workflow",
    position: { x: id === "a" ? 360 : 360, y: id === "a" ? 80 : 260 },
    data: {
      kind: "agent",
      title: id.toUpperCase(),
      description: "",
      agentConfig: {
        schemaVersion: 3,
        executor: { agentCli: "c", modelId: "m" },
        roleId: "",
        skills: [],
        mcps: [],
        prompt: id,
      },
    },
  });

  const nodes: WorkflowDefinitionNode[] = [
    {
      id: "start",
      type: "workflow",
      position: { x: 0, y: 160 },
      data: { kind: "start", title: "Start", description: "" },
    },
    {
      id: "condition",
      type: "workflow",
      position: { x: 180, y: 160 },
      data: {
        kind: "condition",
        title: "Condition",
        description: "",
        cases: [
          {
            id: "a",
            logic: "and",
            conditions: [
              {
                variableSelector: ["route", "choice"],
                operator: "is",
                value: "a",
              },
            ],
          },
        ],
      },
    },
    agent("a"),
    agent("b"),
    {
      id: "aggregator",
      type: "workflow",
      position: { x: 560, y: 160 },
      data: {
        kind: "aggregator",
        title: "Aggregator",
        description: "",
        aggregatorConfig: {
          variables:
            smokeCase === "false"
              ? [["flag", "value"]]
              : [
                  ["a", "output"],
                  ["b", "output"],
                ],
        },
      },
    },
    {
      id: "downstream",
      type: "workflow",
      position: { x: 760, y: 160 },
      data: {
        kind: "output",
        title: "Downstream",
        description: "",
        outputs: [
          {
            name: "result",
            variableSelector: ["aggregator", "output"],
          },
        ],
      },
    },
  ];

  const edges: WorkflowDefinitionEdge[] = [
    { id: "start-condition", source: "start", target: "condition" },
    {
      id: "condition-a",
      source: "condition",
      sourceHandle: "a",
      target: "a",
    },
    {
      id: "condition-b",
      source: "condition",
      sourceHandle: "else",
      target: "b",
    },
    { id: "a-aggregator", source: "a", target: "aggregator" },
    { id: "b-aggregator", source: "b", target: "aggregator" },
    {
      id: "aggregator-downstream",
      source: "aggregator",
      target: "downstream",
    },
  ];

  const globalVariables: WorkflowGlobalVariable[] = [
    {
      name: "route.choice",
      valueType: "string",
      value: smokeCase === "b" ? "b" : "a",
    },
    ...(smokeCase === "false"
      ? [{ name: "flag.value", valueType: "boolean" as const, value: false }]
      : []),
  ];

  return serializeWorkflowGraph({
    nodes,
    edges,
    viewport: { x: 0, y: 0, zoom: 1 },
    globalVariables,
  });
}
