import {
  type CSSProperties,
  type ReactNode,
  useEffect,
  useMemo,
  useRef,
} from "react";
import { useTranslation } from "react-i18next";
import { IconPlus } from "@tabler/icons-react";
import type { Edge, Node } from "@xyflow/react";
import {
  supportsWorkflowNodeScope,
  type WorkflowCapabilities,
  type WorkflowNodeData,
  type WorkflowNodeKind,
} from "@ora/workflow-mock";
import {
  Button,
  cn,
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@ora/ui";
import type { WorkflowContainerInsertion } from "../workflow-container-insertion";
import { WORKFLOW_CANVAS_NODE_KINDS } from "../workflow-canvas-node-kinds";
import { getNodeMetadata } from "../workflow-node-metadata";
import {
  useWorkflowIterationActions,
  type WorkflowIterationActions,
  WorkflowIterationActionsContext,
} from "./iteration-actions-context";

/** Topology that decides insert affordances. Card titles and case text are omitted. */
function iterationActionsStructureKey(
  nodes: readonly Node<WorkflowNodeData, "workflow">[],
  edges: readonly Edge[],
): string {
  const nodePart = nodes
    .map((node) =>
      [
        node.id,
        node.parentId ?? "",
        node.data.kind,
        node.data.containerId ?? "",
      ].join("\u001f"),
    )
    .join("\u001e");
  const edgePart = edges
    .map((edge) =>
      [
        edge.id,
        edge.source,
        edge.sourceHandle ?? "",
        edge.target,
        edge.targetHandle ?? "",
      ].join("\u001f"),
    )
    .join("\u001e");
  return `${nodePart}\u0000${edgePart}`;
}

/** Provides graph-aware iteration authoring actions to custom nodes and edges. */
export function WorkflowIterationActionsProvider({
  capabilities,
  nodes,
  edges,
  readOnly,
  onInsert,
  onToggleCollapsed,
  children,
}: {
  capabilities: WorkflowCapabilities;
  nodes: Node<WorkflowNodeData, "workflow">[];
  edges: Edge[];
  readOnly: boolean;
  onInsert: (
    kind: WorkflowNodeKind,
    insertion: WorkflowContainerInsertion,
  ) => void;
  onToggleCollapsed: (iterationId: string) => void;
  children: ReactNode;
}) {
  const onInsertRef = useRef(onInsert);
  const onToggleCollapsedRef = useRef(onToggleCollapsed);
  useEffect(() => {
    onInsertRef.current = onInsert;
    onToggleCollapsedRef.current = onToggleCollapsed;
  });
  // Node config edits replace the nodes array but do not change where a card
  // can be inserted. Reuse the last topology so every other card stays put.
  const structureKey = iterationActionsStructureKey(nodes, edges);
  const graph = useMemo(
    () => ({ nodes, edges }),
    // eslint-disable-next-line react-hooks/exhaustive-deps -- topology only; config edits must not refresh every card
    [structureKey],
  );
  const value = useMemo<WorkflowIterationActions>(() => {
    const currentNodes = graph.nodes;
    const currentEdges = graph.edges;
    const nodeById = new Map(currentNodes.map((node) => [node.id, node]));
    const iterationIds = new Set(
      currentNodes
        .filter((node) => node.data.kind === "iteration")
        .map((node) => node.id),
    );
    const ownerOf = (nodeId: string): string | null => {
      const parentId = nodeById.get(nodeId)?.parentId;
      return parentId !== undefined && iterationIds.has(parentId)
        ? parentId
        : null;
    };
    return {
      nodeTypes: capabilities.nodeTypes.filter((nodeType) =>
        supportsWorkflowNodeScope(nodeType, "iteration"),
      ),
      loopNodeTypes: capabilities.nodeTypes.filter((nodeType) =>
        supportsWorkflowNodeScope(nodeType, "loop"),
      ),
      canvasNodeTypes: capabilities.nodeTypes.filter(
        (nodeType) =>
          nodeType.kind !== "start" &&
          WORKFLOW_CANVAS_NODE_KINDS.has(nodeType.kind) &&
          supportsWorkflowNodeScope(nodeType, "workflow"),
      ),
      readOnly,
      insertionForEdge: (edge) => {
        // The fixed start already owns the entry add affordance. Repeating it at the
        // entry edge midpoint makes one logical insertion seam look like two actions.
        if (edge.sourceHandle === "iteration-entry") {
          return null;
        }
        const sourceOwner = ownerOf(edge.source);
        const targetOwner = ownerOf(edge.target);
        const iterationId =
          sourceOwner !== null && sourceOwner === targetOwner
            ? sourceOwner
            : targetOwner !== null && edge.source === targetOwner
              ? targetOwner
              : null;
        if (iterationId !== null) {
          return { type: "edge", iterationId, edgeId: edge.id };
        }
        const source = nodeById.get(edge.source);
        const target = nodeById.get(edge.target);
        // Loop/iteration members already have in-container seams. Only top-level
        // canvas edges get the Dify-style midpoint insert.
        if (
          source === undefined ||
          target === undefined ||
          source.parentId !== undefined ||
          target.parentId !== undefined
        ) {
          return null;
        }
        return { type: "canvas-edge", edgeId: edge.id };
      },
      outputInsertion: (nodeId, sourceHandle) => {
        const source = nodeById.get(nodeId);
        const loopId = source?.data.containerId;
        if (
          loopId &&
          nodeById.get(loopId)?.data.kind === "loop" &&
          source?.data.kind !== "output" &&
          source?.data.kind !== "loopExit"
        ) {
          return {
            type: "loop-output",
            loopId,
            sourceId: nodeId,
            sourceHandle,
          };
        }
        const iterationId = ownerOf(nodeId);
        if (iterationId === null) {
          return null;
        }
        const occupied = currentEdges.some(
          (edge) =>
            edge.source === nodeId &&
            (sourceHandle == null
              ? edge.sourceHandle == null
              : edge.sourceHandle === sourceHandle),
        );
        return occupied
          ? null
          : { type: "output", iterationId, sourceId: nodeId, sourceHandle };
      },
      insert: (kind, insertion) => {
        onInsertRef.current(kind, insertion);
      },
      toggleCollapsed: (iterationId) => {
        onToggleCollapsedRef.current(iterationId);
      },
    };
  }, [capabilities.nodeTypes, graph, readOnly]);
  return (
    <WorkflowIterationActionsContext.Provider value={value}>
      {children}
    </WorkflowIterationActionsContext.Provider>
  );
}

/** Renders the capability-filtered node picker shared by entry, edge, and output seams.
 * The trigger mirrors Dify's add-block affordance: a solid blue circle with a white
 * plus. Port-anchored seams keep it decorative (`pointer-events-none` via className)
 * and forward port clicks through the controlled `open` state instead, so connection
 * drags from the underlying handle are never intercepted. */
export function IterationInsertMenu({
  insertion,
  label,
  className,
  side = "right",
  style,
  open,
  onOpenChange,
}: {
  insertion: WorkflowContainerInsertion;
  label: string;
  className?: string;
  side?: "top" | "right" | "bottom" | "left";
  style?: CSSProperties;
  /** Controlled menu state for seams whose port doubles as the trigger. */
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
}) {
  const actions = useWorkflowIterationActions();
  const { readOnly, insert } = actions;
  const nodeTypes =
    insertion.type === "loop-output"
      ? actions.loopNodeTypes
      : insertion.type === "canvas-edge"
        ? actions.canvasNodeTypes
        : actions.nodeTypes;
  const { t } = useTranslation();
  if (readOnly) {
    return null;
  }
  return (
    <DropdownMenu open={open} onOpenChange={onOpenChange}>
      <DropdownMenuTrigger
        render={
          <Button
            type="button"
            variant="default"
            size="icon-xs"
            style={style}
            className={cn(
              "nodrag nopan rounded-full bg-blue-600 text-white shadow-sm hover:bg-blue-700",
              className,
            )}
            aria-label={label}
          />
        }
      >
        <IconPlus className="size-3" />
      </DropdownMenuTrigger>
      <DropdownMenuContent align="center" side={side} className="w-44">
        {nodeTypes.map((nodeType) => {
          const Icon = getNodeMetadata(nodeType.kind).icon;
          return (
            <DropdownMenuItem
              key={nodeType.kind}
              className="gap-2 text-xs"
              onClick={() => insert(nodeType.kind, insertion)}
            >
              <Icon className="size-3.5" />
              {nodeType.label}
            </DropdownMenuItem>
          );
        })}
        {nodeTypes.length === 0 && (
          <p className="px-2 py-3 text-xs text-muted-foreground">
            {t("settings.workflow.iteration.noSupportedNodes")}
          </p>
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
