import type { WorkflowNodeKind } from "@ora/workflow-mock";

// Keep prototype node definitions available for incremental implementation,
// but do not advertise nodes that the current workflow runtime cannot execute.
export const WORKFLOW_CANVAS_NODE_KINDS = new Set<WorkflowNodeKind>([
  "start",
  "agent",
  "condition",
  "aggregator",
  "loop",
  "iteration",
  "output",
]);
