import type { IterationInsertion } from "./workflow-iteration-graph";

export interface LoopOutputInsertion {
  type: "loop-output";
  loopId: string;
  sourceId: string;
  sourceHandle?: string | null;
}

export interface CanvasEdgeInsertion {
  type: "canvas-edge";
  edgeId: string;
}

export type WorkflowContainerInsertion =
  IterationInsertion | LoopOutputInsertion | CanvasEdgeInsertion;
