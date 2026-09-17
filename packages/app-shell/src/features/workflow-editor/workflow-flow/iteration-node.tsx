import { useLayoutEffect } from "react";
import {
  type ControlPosition,
  Handle,
  type Node,
  NodeResizeControl,
  Position,
  ResizeControlVariant,
  useReactFlow,
  useUpdateNodeInternals,
} from "@xyflow/react";
import { useTranslation } from "react-i18next";
import {
  IconArrowsDoubleSeNw,
  IconChevronDown,
  IconChevronUp,
  IconPlayerPlay,
  IconStack2,
  IconTrash,
} from "@tabler/icons-react";
import {
  WORKFLOW_ITERATION_COLLAPSED_HEIGHT,
  WORKFLOW_ITERATION_COLLAPSED_WIDTH,
  WORKFLOW_ITERATION_ENTRY_HANDLE_Y,
  WORKFLOW_ITERATION_HEADER_HEIGHT,
  WORKFLOW_ITERATION_NODE_HEIGHT,
  WORKFLOW_ITERATION_NODE_WIDTH,
  type WorkflowNodeData,
} from "@ora/workflow-mock";
import { cn } from "@ora/ui";
import { IterationInsertMenu } from "./iteration-actions";
import { useWorkflowIterationActions } from "./iteration-actions-context";

const ITERATION_START_SIZE = 36;
const ITERATION_START_LEFT = 24;
/** Dify-style corner affordance: an invisible but forgiving resize hit zone. */
const ITERATION_RESIZE_HANDLE_SIZE = 20;
const ITERATION_RESIZE_HANDLE_INSET = 6;

export interface IterationNodeFrameProps {
  id: string;
  data: WorkflowNodeData;
  selected: boolean;
  deletable?: boolean;
  isInputCandidate: boolean;
  isOutputCandidate: boolean;
  positionAbsoluteX: number;
  positionAbsoluteY: number;
  nodeKindLabel: string;
}

/** Owns the complete editor presentation of one iteration composite region. */
export function IterationNodeFrame({
  id,
  data,
  selected,
  deletable,
  isInputCandidate,
  isOutputCandidate,
  positionAbsoluteX,
  positionAbsoluteY,
  nodeKindLabel,
}: IterationNodeFrameProps) {
  const { t } = useTranslation();
  const { deleteElements, getNode } =
    useReactFlow<Node<WorkflowNodeData, "workflow">>();
  const updateNodeInternals = useUpdateNodeInternals();
  const iterationActions = useWorkflowIterationActions();
  const collapsed = data.collapsed === true;
  const memberCount =
    typeof data.regionMemberCount === "number" ? data.regionMemberCount : 0;
  const frame = getNode(id);
  // Manual resizes rewrite initialWidth/initialHeight on every gesture frame, so
  // the persisted size and the rendered frame never drift apart mid-drag.
  const expandedWidth = Math.max(
    WORKFLOW_ITERATION_NODE_WIDTH,
    frame?.initialWidth ?? WORKFLOW_ITERATION_NODE_WIDTH,
  );
  const expandedHeight = Math.max(
    WORKFLOW_ITERATION_NODE_HEIGHT,
    frame?.initialHeight ?? WORKFLOW_ITERATION_NODE_HEIGHT,
  );

  useLayoutEffect(() => {
    // The internal start handle appears only in expanded mode. Refreshing after the DOM commit
    // keeps React Flow's handle registry aligned with the presentation-only composite chrome.
    updateNodeInternals(id);
  }, [collapsed, expandedHeight, expandedWidth, id, updateNodeInternals]);

  return (
    <div
      data-workflow-node=""
      data-workflow-node-id={id}
      data-x={String(Math.round(positionAbsoluteX))}
      data-y={String(Math.round(positionAbsoluteY))}
      data-workflow-iteration-frame=""
      data-collapsed={collapsed}
      aria-label={`${data.title}: ${nodeKindLabel}`}
      className={cn(
        "relative overflow-visible rounded-2xl border bg-card/80 shadow-sm transition-[border-color,box-shadow,background-color]",
        selected
          ? "border-ring shadow-md ring-2 ring-ring/10"
          : "border-violet-500/40 bg-violet-500/[0.035]",
        isOutputCandidate && "border-ring/70 ring-2 ring-ring/10",
      )}
      style={{
        width: collapsed ? WORKFLOW_ITERATION_COLLAPSED_WIDTH : expandedWidth,
        height: collapsed
          ? WORKFLOW_ITERATION_COLLAPSED_HEIGHT
          : expandedHeight,
      }}
    >
      <Handle
        type="target"
        position={Position.Left}
        data-workflow-input={id}
        aria-label={t("settings.workflow.connectTo", { name: data.title })}
        className={cn(
          "workflow-port workflow-port-input !size-2.5 !border-0 !bg-transparent",
          isInputCandidate && "workflow-port-candidate",
        )}
        style={{ top: WORKFLOW_ITERATION_HEADER_HEIGHT / 2 }}
      />
      <Handle
        type="source"
        position={Position.Right}
        data-workflow-output={id}
        aria-label={t("settings.workflow.connectFrom", { name: data.title })}
        className={cn(
          "workflow-port workflow-port-output !size-2.5 !border-0 !bg-transparent",
          isOutputCandidate && "workflow-port-candidate",
        )}
        style={{ top: WORKFLOW_ITERATION_HEADER_HEIGHT / 2 }}
      />

      <div
        className="relative z-20 flex items-center gap-2 border-b border-violet-500/20 px-3"
        style={{ height: WORKFLOW_ITERATION_HEADER_HEIGHT }}
      >
        <span className="flex size-7 shrink-0 items-center justify-center rounded-lg bg-violet-500/12 text-violet-700 dark:text-violet-300">
          <IconStack2 className="size-4" />
        </span>
        <span className="min-w-0 flex-1 truncate text-sm font-semibold">
          {data.title}
        </span>
        <span className="shrink-0 rounded-md bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground">
          {t("settings.workflow.iteration.regionSummary", {
            total: memberCount,
          })}
        </span>
        <button
          type="button"
          className="nodrag nopan flex size-7 shrink-0 items-center justify-center rounded-md text-muted-foreground outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring"
          aria-label={t(
            collapsed
              ? "settings.workflow.iteration.expand"
              : "settings.workflow.iteration.collapse",
          )}
          title={t(
            collapsed
              ? "settings.workflow.iteration.expand"
              : "settings.workflow.iteration.collapse",
          )}
          onClick={() => {
            iterationActions.toggleCollapsed(id);
          }}
        >
          {collapsed ? (
            <IconChevronUp className="size-4" />
          ) : (
            <IconChevronDown className="size-4" />
          )}
        </button>
        {selected && deletable ? (
          <button
            type="button"
            className="nodrag nopan flex size-7 shrink-0 items-center justify-center rounded-md text-muted-foreground outline-none hover:bg-destructive/10 hover:text-destructive focus-visible:ring-2 focus-visible:ring-ring"
            aria-label={t("settings.workflow.deleteNamed", {
              name: data.title,
            })}
            onClick={() => {
              void deleteElements({ nodes: [{ id }] });
            }}
          >
            <IconTrash className="size-3.5" />
          </button>
        ) : undefined}
      </div>

      {!collapsed && (
        <>
          <div
            data-workflow-iteration-region=""
            className="pointer-events-none absolute inset-x-3 bottom-3"
            style={{ top: WORKFLOW_ITERATION_HEADER_HEIGHT + 8 }}
          >
            {memberCount === 0 ? (
              <span className="pointer-events-none absolute right-3 top-2 text-[11px] text-muted-foreground">
                {t("settings.workflow.iteration.emptyHint")}
              </span>
            ) : undefined}
          </div>

          <div
            className="nodrag nopan group/iteration-start absolute z-20 flex items-center gap-1.5"
            style={{
              left: ITERATION_START_LEFT,
              top: WORKFLOW_ITERATION_ENTRY_HANDLE_Y - ITERATION_START_SIZE / 2,
            }}
          >
            <div
              data-workflow-iteration-start=""
              role="img"
              aria-label={t("settings.workflow.iteration.internalStart")}
              title={t("settings.workflow.iteration.internalStart")}
              className="relative flex size-9 shrink-0 items-center justify-center rounded-full border border-violet-500/35 bg-background text-violet-700 shadow-sm dark:text-violet-300"
            >
              <IconPlayerPlay className="size-4" />
              <Handle
                id="iteration-entry"
                type="source"
                position={Position.Right}
                data-workflow-iteration-entry={id}
                aria-label={t("settings.workflow.iteration.entryHandle", {
                  name: data.title,
                })}
                className="workflow-port workflow-port-output !size-2.5 !border-0 !bg-transparent"
              />
            </div>
            {/* Dify-style affordance: the entry plus stays out of sight until the
                author hovers the internal start row (or opens it from the keyboard). */}
            <IterationInsertMenu
              insertion={{ type: "entry", iterationId: id }}
              label={t("settings.workflow.iteration.addNode")}
              className="pointer-events-none opacity-0 transition-opacity duration-150 group-hover/iteration-start:pointer-events-auto group-hover/iteration-start:opacity-100 focus-visible:pointer-events-auto focus-visible:opacity-100 data-popup-open:pointer-events-auto data-popup-open:opacity-100"
            />
          </div>
          {!iterationActions.readOnly && (
            <NodeResizeControl
              variant={ResizeControlVariant.Handle}
              position={"bottom-right" satisfies ControlPosition}
              minWidth={WORKFLOW_ITERATION_NODE_WIDTH}
              minHeight={WORKFLOW_ITERATION_NODE_HEIGHT}
              className="group/iteration-resize"
              style={{
                left: "auto",
                top: "auto",
                right: ITERATION_RESIZE_HANDLE_INSET,
                bottom: ITERATION_RESIZE_HANDLE_INSET,
                width: ITERATION_RESIZE_HANDLE_SIZE,
                height: ITERATION_RESIZE_HANDLE_SIZE,
                // The built-in handle pins itself to the frame corner with a
                // translated 5px dot; a larger in-corner zone keeps the gesture
                // forgiving while staying clear of the rounded border.
                translate: "none",
                border: "none",
                backgroundColor: "transparent",
              }}
            >
              <IconArrowsDoubleSeNw className="size-3.5 text-violet-600 opacity-0 transition-opacity duration-150 group-hover/iteration-resize:opacity-100 dark:text-violet-300" />
            </NodeResizeControl>
          )}
        </>
      )}
    </div>
  );
}
