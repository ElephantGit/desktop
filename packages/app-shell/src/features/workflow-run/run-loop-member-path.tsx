import { useTranslation } from "react-i18next";
import { cn } from "@ora/ui";
import type {
  GraphWorkflowRound,
  GraphWorkflowRun,
} from "@ora/workflow-runtime";
import { RunStatusMark } from "./run-status-mark";
import { runStatusTone } from "./run-status-style";

/** Keeps branch alternatives out of the active round path without hiding their inspection entry. */
export function RunLoopMemberPath({
  run,
  round,
  memberIds,
  primaryId,
  onFocusNode,
}: {
  run: GraphWorkflowRun;
  round: GraphWorkflowRound;
  memberIds: string[];
  primaryId: string | null;
  onFocusNode: (id: string) => void;
}) {
  const { t } = useTranslation();
  const nodes = memberIds.flatMap((id) => {
    const node = run.definitionSnapshot.nodes.find((node) => node.id === id);
    return node ? [node] : [];
  });
  const recorded = nodes.filter((node) => {
    const state = round.nodeStates[node.id];
    return (
      state !== undefined &&
      state.status !== "inactive" &&
      state.status !== "idle"
    );
  });
  const remaining = nodes.filter((node) => !recorded.includes(node));
  const renderNode = (node: (typeof nodes)[number]) => {
    const state = round.nodeStates[node.id];
    const status = state?.status ?? "inactive";
    const label = recorded.includes(node)
      ? t(runStatusTone(status).labelKey)
      : t("workflowRun.theater.notRunThisRound");
    return (
      <button
        key={node.id}
        type="button"
        data-path-node={node.id}
        aria-current={primaryId === node.id ? "step" : undefined}
        aria-label={`${node.data.title}: ${label}`}
        onClick={() => onFocusNode(node.id)}
        className={cn(
          "flex min-w-0 items-center gap-2 rounded-md border px-3 py-2 text-left text-xs",
          primaryId === node.id
            ? "border-violet-500/50 bg-violet-500/10"
            : "border-border/60 bg-background hover:bg-muted",
        )}
      >
        <RunStatusMark status={status} quiet />
        <span className="min-w-0 break-words">{node.data.title}</span>
      </button>
    );
  };
  return (
    <div className="space-y-2 border-t border-border/60 pt-3">
      <p className="text-xs text-muted-foreground">
        {t("workflowRun.loopView.recordedNodes")}
      </p>
      <div
        className="grid grid-cols-2 gap-2 sm:grid-cols-3"
        aria-label={t("workflowRun.loopView.recordedNodes")}
      >
        {recorded.map(renderNode)}
      </div>
      {remaining.length > 0 && (
        <details key={round.id} className="text-xs text-muted-foreground">
          <summary className="cursor-pointer py-1.5">
            {t("workflowRun.loopView.unexecutedNodes", {
              count: remaining.length,
            })}
          </summary>
          <div className="grid grid-cols-2 gap-2 pt-1 sm:grid-cols-3">
            {remaining.map(renderNode)}
          </div>
        </details>
      )}
    </div>
  );
}
