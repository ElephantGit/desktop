import { Fragment, memo } from "react";
import {
  Handle,
  Position,
  useReactFlow,
  type Node,
  type NodeProps,
} from "@xyflow/react";
import { useTranslation } from "react-i18next";
import { IconTrash } from "@tabler/icons-react";
import { cn } from "@ora/ui";
import { IconChevronDown, IconChevronUp } from "@tabler/icons-react";
import {
  createMockWorkflowCapabilities,
  createMockWorkflowNodeType,
  isWorkflowConditionComparisonComplete,
  resolveConditionCases,
  WORKFLOW_ITERATION_CARD_WIDTH,
  WORKFLOW_ITERATION_ENTRY_HANDLE_Y,
  WORKFLOW_ITERATION_NODE_HEIGHT,
  WORKFLOW_ITERATION_NODE_WIDTH,
  WORKFLOW_NODE_ANCHOR_Y,
  WORKFLOW_NODE_WIDTH,
  type WorkflowNodeData,
} from "@ora/workflow-mock";
import {
  AgentExecutionModeMark,
  WorkflowNodeCardShell,
} from "../../workflow-node-chrome";
import { useWorkflowConnectionState } from "./use-connection-state";
import { WorkflowNodeParameterSummary } from "./node-parameter-summary";
import { IterationInsertMenu } from "./iteration-actions";
import { useWorkflowIterationActions } from "./iteration-actions-context";
import type { IterationInsertion } from "../workflow-iteration-graph";

const CONDITION_NODE_WIDTH = 320;
const CONDITION_FIRST_HANDLE_Y = 82;
const CONDITION_EMPTY_CASE_HEIGHT = 28;
const CONDITION_RULE_HEIGHT = 44;
const CONDITION_UNSET_RULE_HEIGHT = 28;
const CONDITION_LOGIC_CONNECTOR_HEIGHT = 14;

/** Renders one workflow card with left/right handles styled for the definition editor. */
export const WorkflowFlowNodeView = memo(function WorkflowFlowNodeView({
  id,
  data,
  deletable,
  selected,
  parentId,
  positionAbsoluteX,
  positionAbsoluteY,
}: NodeProps<Node<WorkflowNodeData, "workflow">>) {
  const { i18n, t } = useTranslation();
  const { deleteElements } = useReactFlow<Node<WorkflowNodeData, "workflow">>();
  const { connectionCandidateEndpoint, connectionCandidateNodeId } =
    useWorkflowConnectionState();
  const locale =
    i18n.resolvedLanguage === "en-US" ? ("en-US" as const) : ("zh-CN" as const);
  const nodeKindLabel = createMockWorkflowNodeType(data.kind, locale).label;
  const isConnectionCandidate = connectionCandidateNodeId === id;
  const isInputCandidate =
    isConnectionCandidate && connectionCandidateEndpoint === "target";
  const isOutputCandidate =
    isConnectionCandidate && connectionCandidateEndpoint === "source";
  const conditionCases =
    data.kind === "condition" ? resolveConditionCases(data) : [];
  const iterationActions = useWorkflowIterationActions();

  if (data.kind === "iteration") {
    return (
      <IterationNodeFrame
        id={id}
        data={data}
        selected={selected}
        deletable={deletable}
        isInputCandidate={isInputCandidate}
        isOutputCandidate={isOutputCandidate}
        positionAbsoluteX={positionAbsoluteX}
        positionAbsoluteY={positionAbsoluteY}
        nodeKindLabel={nodeKindLabel}
      />
    );
  }

  return (
    <WorkflowNodeCardShell
      data-workflow-node=""
      data-workflow-node-id={id}
      data-x={String(Math.round(positionAbsoluteX))}
      data-y={String(Math.round(positionAbsoluteY))}
      kind={data.kind}
      title={data.title}
      description={data.description}
      kindLabel={id}
      density="editor"
      selected={selected}
      width={
        data.kind === "condition" ? CONDITION_NODE_WIDTH : WORKFLOW_NODE_WIDTH
      }
      titleAccessory={
        data.kind === "agent" ? (
          <AgentExecutionModeMark
            interactive={data.agentConfig?.interactive === true}
          />
        ) : undefined
      }
      ariaLabel={`${t("settings.workflow.nodeSuffix", { type: nodeKindLabel })}: ${data.title}`}
      frameClassName={cn(
        isConnectionCandidate && "border-ring/60 shadow-md ring-2 ring-ring/10",
      )}
      details={
        data.kind === "condition" ? (
          <ConditionNodeDetails data={data} locale={locale} />
        ) : (
          <WorkflowNodeParameterSummary data={data} />
        )
      }
      detailsClassName={data.kind === "condition" ? "space-y-2" : undefined}
      headerEnd={
        selected && deletable ? (
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
        ) : undefined
      }
      targetHandle={
        <Handle
          type="target"
          position={Position.Left}
          data-workflow-input={id}
          aria-label={t("settings.workflow.connectTo", { name: data.title })}
          className={cn(
            "workflow-port workflow-port-input !size-2.5 !border-0 !bg-transparent",
            isInputCandidate && "workflow-port-candidate",
          )}
          style={{ top: WORKFLOW_NODE_ANCHOR_Y }}
        />
      }
      sourceHandle={
        data.kind === "condition" ? (
          <>
            {conditionCases.map((conditionCase, index) => (
              <Fragment key={conditionCase.id}>
                <Handle
                  id={conditionCase.id}
                  type="source"
                  position={Position.Right}
                  data-workflow-output={id}
                  aria-label={`${t("settings.workflow.connectFrom", { name: data.title })} · ${conditionCase.id}`}
                  className={cn(
                    "workflow-port workflow-port-output !size-2.5 !border-0 !bg-transparent",
                    isOutputCandidate && "workflow-port-candidate",
                  )}
                  style={{
                    top: conditionHandleTop(conditionCases, index),
                  }}
                />
                <IterationOutputInsertButton
                  insertion={iterationActions.outputInsertion(
                    id,
                    conditionCase.id,
                  )}
                  top={conditionHandleTop(conditionCases, index)}
                  label={t("settings.workflow.iteration.appendBranch", {
                    branch: conditionCase.id,
                  })}
                />
              </Fragment>
            ))}
            <Handle
              id="else"
              type="source"
              position={Position.Right}
              data-workflow-output={id}
              aria-label={`${t("settings.workflow.connectFrom", { name: data.title })} · else`}
              className={cn(
                "workflow-port workflow-port-output !size-2.5 !border-0 !bg-transparent",
                isOutputCandidate && "workflow-port-candidate",
              )}
              style={{
                top: conditionHandleTop(conditionCases, conditionCases.length),
              }}
            />
            <IterationOutputInsertButton
              insertion={iterationActions.outputInsertion(id, "else")}
              top={conditionHandleTop(conditionCases, conditionCases.length)}
              label={t("settings.workflow.iteration.appendBranch", {
                branch: "ELSE",
              })}
            />
          </>
        ) : (
          <>
            <Handle
              type="source"
              position={Position.Right}
              data-workflow-output={id}
              aria-label={t("settings.workflow.connectFrom", {
                name: data.title,
              })}
              className={cn(
                "workflow-port workflow-port-output !size-2.5 !border-0 !bg-transparent",
                isOutputCandidate && "workflow-port-candidate",
              )}
              style={{ top: WORKFLOW_NODE_ANCHOR_Y }}
            />
            {parentId !== undefined && (
              <IterationOutputInsertButton
                insertion={iterationActions.outputInsertion(id)}
                top={WORKFLOW_NODE_ANCHOR_Y}
                label={t("settings.workflow.iteration.appendOutput", {
                  name: data.title,
                })}
              />
            )}
          </>
        )
      }
    />
  );
});

/** Renders Dify-style IF / ELIF / ELSE rows whose labels align with branch handles. */
function ConditionNodeDetails({
  data,
  locale,
}: {
  data: WorkflowNodeData;
  locale: "zh-CN" | "en-US";
}) {
  const { t } = useTranslation();
  const cases = resolveConditionCases(data);
  const operators = createMockWorkflowCapabilities(locale).conditionOperators;
  return (
    <div className="space-y-2">
      {cases.map((conditionCase, caseIndex) =>
        conditionCase.conditions.length === 0 ? (
          <div
            key={conditionCase.id}
            className="flex h-5 items-center justify-end text-[10px] font-semibold"
          >
            {caseIndex === 0 ? "IF" : "ELIF"}
          </div>
        ) : (
          <div key={conditionCase.id} className="space-y-1">
            <div className="flex items-center justify-between text-[10px] font-semibold text-muted-foreground">
              <span>CASE {caseIndex + 1}</span>
              <span className="text-foreground">
                {caseIndex === 0 ? "IF" : "ELIF"}
              </span>
            </div>
            <div>
              {conditionCase.conditions.map((comparison, comparisonIndex) => (
                <Fragment key={comparisonIndex}>
                  {comparisonIndex > 0 && (
                    <div className="flex h-3.5 items-center justify-end pr-2 text-[10px] font-semibold text-blue-600 dark:text-blue-400">
                      {(conditionCase.logic ?? "and").toUpperCase()}
                    </div>
                  )}
                  <div className="min-w-0 rounded-lg bg-muted/70 px-2 py-1.5 text-[10px] leading-4">
                    {isWorkflowConditionComparisonComplete(comparison) ? (
                      <>
                        <div className="flex min-w-0 items-center gap-1 font-medium">
                          <span className="truncate text-muted-foreground">
                            {comparison.variableSelector[0]}
                          </span>
                          <span className="text-blue-600 dark:text-blue-400">
                            /
                          </span>
                          <span className="truncate text-blue-600 dark:text-blue-400">
                            {comparison.variableSelector.slice(1).join(".")}
                          </span>
                        </div>
                        <div className="flex min-w-0 items-center gap-2">
                          <span className="font-semibold">
                            {operators.find(
                              (operator) =>
                                operator.value === comparison.operator,
                            )?.label ?? comparison.operator}
                          </span>
                          <span className="truncate text-muted-foreground">
                            {conditionValueLabel(comparison.value)}
                          </span>
                        </div>
                      </>
                    ) : (
                      <span className="font-medium text-muted-foreground">
                        {t("settings.workflow.condition.unset")}
                      </span>
                    )}
                  </div>
                </Fragment>
              ))}
            </div>
          </div>
        ),
      )}
      <div className="flex justify-end pt-0.5 text-[10px] font-semibold">
        ELSE
      </div>
    </div>
  );
}

/** Aligns each branch handle with its rendered IF / ELIF / ELSE label. */
/** Renders the iteration composite as an embedded container frame (ADR D7).
 *
 * The card sits on top of a dashed region drop zone; nodes dragged into the zone become
 * region members (parentId containment) and execute once per round. The entry handle inside
 * the zone feeds the first round members; the right-side handle is the outer exit.
 */
function IterationNodeFrame({
  id,
  data,
  selected,
  deletable,
  isInputCandidate,
  isOutputCandidate,
  positionAbsoluteX,
  positionAbsoluteY,
  nodeKindLabel,
}: {
  id: string;
  data: WorkflowNodeData;
  selected: boolean;
  deletable?: boolean;
  isInputCandidate: boolean;
  isOutputCandidate: boolean;
  positionAbsoluteX: number;
  positionAbsoluteY: number;
  nodeKindLabel: string;
}) {
  const { t } = useTranslation();
  const { deleteElements, getNode } =
    useReactFlow<Node<WorkflowNodeData, "workflow">>();
  const iterationActions = useWorkflowIterationActions();
  const collapsed = data.collapsed === true;
  const memberCount =
    typeof data.regionMemberCount === "number" ? data.regionMemberCount : 0;
  const frame = getNode(id);
  const expandedWidth = Math.max(
    WORKFLOW_ITERATION_NODE_WIDTH,
    frame?.initialWidth ?? WORKFLOW_ITERATION_NODE_WIDTH,
  );
  const expandedHeight = Math.max(
    WORKFLOW_ITERATION_NODE_HEIGHT,
    frame?.initialHeight ?? WORKFLOW_ITERATION_NODE_HEIGHT,
  );
  return (
    <div
      data-workflow-node=""
      data-workflow-node-id={id}
      data-x={String(Math.round(positionAbsoluteX))}
      data-y={String(Math.round(positionAbsoluteY))}
      data-workflow-iteration-frame=""
      data-collapsed={collapsed}
      className={cn(
        "relative rounded-2xl border-2 border-dashed transition-colors",
        selected
          ? "border-ring/70 bg-ring/5"
          : "border-violet-500/40 bg-violet-500/5",
      )}
      style={{
        width: collapsed ? WORKFLOW_ITERATION_CARD_WIDTH + 16 : expandedWidth,
        height: collapsed ? 112 : expandedHeight,
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
        style={{ top: WORKFLOW_NODE_ANCHOR_Y }}
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
        style={{ top: WORKFLOW_NODE_ANCHOR_Y }}
      />
      <div className="p-2">
        <WorkflowNodeCardShell
          kind={data.kind}
          title={data.title}
          description={data.description}
          kindLabel={nodeKindLabel}
          density="editor"
          selected={selected}
          width={WORKFLOW_ITERATION_CARD_WIDTH}
          details={<WorkflowNodeParameterSummary data={data} />}
          ariaLabel={`${data.title}: ${nodeKindLabel}`}
          frameClassName={cn(
            isOutputCandidate && "border-ring/60 shadow-md ring-2 ring-ring/10",
          )}
          headerEnd={
            <div className="flex items-center gap-1">
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
          }
        />
      </div>
      {collapsed ? (
        <span className="pointer-events-none absolute bottom-1.5 right-3 text-[10px] text-muted-foreground">
          {t("settings.workflow.iteration.regionSummary", {
            total: memberCount,
          })}
        </span>
      ) : undefined}
      {!collapsed && (
        <div className="absolute inset-x-3 bottom-3 top-[116px] rounded-xl border border-dashed border-violet-500/30">
          <div className="pointer-events-none absolute left-3 top-2 text-[11px] text-muted-foreground">
            {memberCount > 0
              ? t("settings.workflow.iteration.regionSummary", {
                  total: memberCount,
                })
              : t("settings.workflow.iteration.emptyHint")}
          </div>
          <div className="absolute left-3 top-9 flex items-center gap-2">
            <span
              className="pointer-events-none flex size-5 items-center justify-center rounded-full border border-violet-500/40 bg-violet-500/10 text-[9px] font-semibold text-violet-700 dark:text-violet-300"
              aria-hidden
            >
              ▶
            </span>
            <span className="pointer-events-none text-[10px] font-medium text-violet-700 dark:text-violet-300">
              {t("settings.workflow.iteration.internalStart")}
            </span>
            <IterationInsertMenu
              insertion={{ type: "entry", iterationId: id }}
              label={t("settings.workflow.iteration.addNode")}
            />
          </div>
        </div>
      )}
      {!collapsed ? (
        <Handle
          id="iteration-entry"
          type="source"
          position={Position.Left}
          data-workflow-iteration-entry={id}
          aria-label={t("settings.workflow.iteration.entryHandle", {
            name: data.title,
          })}
          className="workflow-port workflow-port-output !size-2.5 !border-0 !bg-transparent"
          style={{ top: WORKFLOW_ITERATION_ENTRY_HANDLE_Y, left: 14 }}
        />
      ) : undefined}
    </div>
  );
}

/** Shows an append affordance only for an unconnected region output. */
function IterationOutputInsertButton({
  insertion,
  top,
  label,
}: {
  insertion: IterationInsertion | null;
  top: number;
  label: string;
}) {
  if (insertion === null) {
    return null;
  }
  return (
    <div
      className="absolute z-10"
      style={{ right: -34, top, transform: "translateY(-50%)" }}
    >
      <IterationInsertMenu insertion={insertion} label={label} side="right" />
    </div>
  );
}

function conditionHandleTop(
  cases: ReturnType<typeof resolveConditionCases>,
  branchIndex: number,
): number {
  return cases
    .slice(0, branchIndex)
    .reduce(
      (top, conditionCase) =>
        top +
        (conditionCase.conditions.length === 0
          ? CONDITION_EMPTY_CASE_HEIGHT
          : CONDITION_EMPTY_CASE_HEIGHT +
            conditionCase.conditions.reduce(
              (height, comparison) =>
                height +
                (isWorkflowConditionComparisonComplete(comparison)
                  ? CONDITION_RULE_HEIGHT
                  : CONDITION_UNSET_RULE_HEIGHT),
              0,
            ) +
            Math.max(0, conditionCase.conditions.length - 1) *
              CONDITION_LOGIC_CONNECTOR_HEIGHT),
      CONDITION_FIRST_HANDLE_Y,
    );
}

/** Formats a condition comparison value for the compact canvas card. */
function conditionValueLabel(value: unknown): string {
  if (value === undefined || value === null) {
    return "";
  }
  return typeof value === "string" ? value : JSON.stringify(value);
}
