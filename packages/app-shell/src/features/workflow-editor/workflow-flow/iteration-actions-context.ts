import { createContext, useContext } from "react";
import type { Edge } from "@xyflow/react";
import type { WorkflowNodeKind, WorkflowNodeType } from "@ora/workflow-mock";

import type { WorkflowContainerInsertion } from "../workflow-container-insertion";

export interface WorkflowIterationActions {
  loopNodeTypes: WorkflowNodeType[];
  canvasNodeTypes: WorkflowNodeType[];
  nodeTypes: WorkflowNodeType[];
  readOnly: boolean;
  insertionForEdge: (
    edge: Pick<Edge, "id" | "source" | "sourceHandle" | "target">,
  ) => WorkflowContainerInsertion | null;
  outputInsertion: (
    nodeId: string,
    sourceHandle?: string | null,
  ) => WorkflowContainerInsertion | null;
  insert: (
    kind: WorkflowNodeKind,
    insertion: WorkflowContainerInsertion,
  ) => void;
  toggleCollapsed: (iterationId: string) => void;
}

export const WorkflowIterationActionsContext =
  createContext<WorkflowIterationActions | null>(null);

/** Reads iteration graph actions from the workflow canvas. */
export function useWorkflowIterationActions(): WorkflowIterationActions {
  const value = useContext(WorkflowIterationActionsContext);
  if (value === null) {
    throw new Error(
      "useWorkflowIterationActions requires WorkflowIterationActionsProvider",
    );
  }
  return value;
}
