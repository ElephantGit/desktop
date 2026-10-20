import { memo, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  BaseEdge,
  EdgeLabelRenderer,
  type EdgeProps,
  getBezierPath,
} from "@xyflow/react";
import { cn } from "@ora/ui";
import { IterationInsertMenu } from "./iteration-actions";
import { useWorkflowIterationActions } from "./iteration-actions-context";
import { WORKFLOW_EDGE_CONTROL_Z_INDEX } from "./z-index";

const CANVAS_INSERT_HOVER_HOLD_MS = 180;

/** Draws a selectable workflow edge with an accessible hit target and optional branch label. */
export const WorkflowFlowEdgeView = memo(function WorkflowFlowEdgeView({
  id,
  source,
  sourceHandleId,
  target,
  sourceX,
  sourceY,
  sourcePosition,
  targetX,
  targetY,
  targetPosition,
  label,
  selected,
  markerEnd,
  style,
  interactionWidth,
}: EdgeProps) {
  const { t } = useTranslation();
  const { insertionForEdge } = useWorkflowIterationActions();
  const insertion = insertionForEdge({
    id,
    source,
    sourceHandle: sourceHandleId,
    target,
  });
  const [hovered, setHovered] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const hideHoverTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(
    () => () => {
      if (hideHoverTimer.current !== null) {
        clearTimeout(hideHoverTimer.current);
      }
    },
    [],
  );
  const showHover = () => {
    if (hideHoverTimer.current !== null) {
      clearTimeout(hideHoverTimer.current);
      hideHoverTimer.current = null;
    }
    setHovered(true);
  };
  const hideHoverSoon = () => {
    if (hideHoverTimer.current !== null) {
      clearTimeout(hideHoverTimer.current);
    }
    hideHoverTimer.current = setTimeout(() => {
      setHovered(false);
      hideHoverTimer.current = null;
    }, CANVAS_INSERT_HOVER_HOLD_MS);
  };
  const [edgePath, labelX, labelY] = getBezierPath({
    sourceX,
    sourceY,
    sourcePosition,
    targetX,
    targetY,
    targetPosition,
  });
  const edgeColor = selected
    ? "var(--ring)"
    : "color-mix(in oklch, var(--foreground) 46%, transparent)";
  const showInsert =
    insertion !== null &&
    (insertion.type !== "canvas-edge" || hovered || selected || menuOpen);
  const insertLabel =
    insertion?.type === "canvas-edge"
      ? t("settings.workflow.insertOnEdge")
      : t("settings.workflow.iteration.insertOnEdge");

  return (
    <>
      <BaseEdge
        id={id}
        path={edgePath}
        markerEnd={markerEnd}
        interactionWidth={interactionWidth}
        style={{
          ...style,
          strokeWidth: selected ? 3 : 2,
          stroke: edgeColor,
        }}
      />
      <path
        d={edgePath}
        fill="none"
        stroke="transparent"
        strokeWidth={interactionWidth ?? 20}
        className="react-flow__edge-interaction"
        onMouseEnter={showHover}
        onMouseLeave={hideHoverSoon}
      />
      {selected && (
        <g className="pointer-events-none">
          {[
            { x: sourceX, y: sourceY },
            { x: targetX, y: targetY },
          ].map((endpoint) => (
            <g key={`${endpoint.x}-${endpoint.y}`}>
              <circle
                cx={endpoint.x}
                cy={endpoint.y}
                r={8}
                fill="var(--background)"
                stroke="var(--ring)"
                strokeWidth={2}
              />
              <circle
                cx={endpoint.x}
                cy={endpoint.y}
                r={4.5}
                fill="var(--foreground)"
              />
            </g>
          ))}
        </g>
      )}
      <EdgeLabelRenderer>
        {showInsert && insertion !== null && (
          <div
            className="nodrag nopan pointer-events-auto absolute"
            style={{
              transform: `translate(-50%, -50%) translate(${labelX}px, ${labelY}px)`,
              // Edges that touch a parented member render above the node layer, so
              // the insert control must explicitly stack above every edge and node
              // elevation or the edge hit target swallows its clicks.
              zIndex: WORKFLOW_EDGE_CONTROL_Z_INDEX,
            }}
            onMouseEnter={showHover}
            onMouseLeave={hideHoverSoon}
            onClick={(event) => event.stopPropagation()}
          >
            <IterationInsertMenu
              insertion={insertion}
              label={insertLabel}
              side="top"
              open={menuOpen}
              onOpenChange={setMenuOpen}
            />
          </div>
        )}
        {label !== undefined && label !== null && label !== "" && (
          <div
            className={cn(
              "nodrag nopan pointer-events-none absolute text-[10px] text-muted-foreground",
              selected && "text-foreground",
            )}
            style={{
              transform: `translate(-50%, -50%) translate(${labelX}px, ${
                labelY - 14
              }px)`,
            }}
          >
            {String(label)}
          </div>
        )}
      </EdgeLabelRenderer>
    </>
  );
});
