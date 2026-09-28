import { useTranslation } from "react-i18next";
import { resolveConditionCases } from "@ora/workflow-mock";
import type {
  GraphWorkflowNodeState,
  WorkflowNodeData,
} from "@ora/workflow-runtime";
import {
  conditionBranchesSummary,
  createWorkflowSummaryLabels,
} from "../workflow-node-chrome";

/** Conditions route execution; their scheduler decisions are not Agent output or instructions. */
export function RunConditionStage({
  data,
  state,
}: {
  data: WorkflowNodeData;
  state: GraphWorkflowNodeState;
}) {
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage === "en-US" ? "en-US" : "zh-CN";
  const labels = createWorkflowSummaryLabels(locale);
  const cases = resolveConditionCases(data);
  const selected =
    state.status === "succeeded" ? state.selectedBranchId : undefined;
  return (
    <div className="mt-5 space-y-3">
      <section className="rounded-xl border border-border/80 bg-muted/30 px-4 py-3">
        <h3 className="text-xs text-muted-foreground">
          {t("workflowRun.conditionView.conditions")}
        </h3>
        <ol className="mt-2 space-y-3 text-sm">
          {cases.map((branch, index) => (
            <li
              key={branch.id}
              className={
                selected === branch.id
                  ? "rounded-lg border border-emerald-500/40 bg-emerald-500/10 p-2"
                  : undefined
              }
            >
              <span className="text-xs font-semibold">
                {index === 0 ? "IF" : "ELSE IF"} · {branch.id}
                {selected === branch.id && (
                  <span className="ml-2 text-emerald-700">
                    {t("workflowRun.conditionView.matched")}
                  </span>
                )}
              </span>
              <p className="mt-1 whitespace-pre-wrap break-words text-foreground/90">
                {conditionBranchesSummary(
                  { ...data, cases: [branch], conditionCases: [branch] },
                  labels,
                  locale,
                )}
              </p>
            </li>
          ))}
          {cases.length === 0 && <li>{data.condition ?? "—"}</li>}
          <li
            className={
              selected === "else"
                ? "rounded-lg border border-emerald-500/40 bg-emerald-500/10 p-2"
                : undefined
            }
          >
            <span className="text-xs font-semibold">ELSE</span>
            {selected === "else" && (
              <span className="ml-2 text-xs text-emerald-700">
                {t("workflowRun.conditionView.matched")}
              </span>
            )}
            <p className="mt-1 text-muted-foreground">
              {t("workflowRun.conditionView.else")}
            </p>
          </li>
        </ol>
      </section>
      <section className="rounded-xl border border-border/80 bg-muted/30 px-4 py-3">
        <h3 className="text-xs text-muted-foreground">
          {t("workflowRun.conditionView.result")}
        </h3>
        <p className="mt-2 text-sm text-muted-foreground">
          {selected !== undefined
            ? t("workflowRun.conditionView.selected", {
                branch: selected === "else" ? "ELSE" : selected,
              })
            : t(
                state.status === "succeeded"
                  ? "workflowRun.conditionView.unavailable"
                  : state.status === "inactive" || state.status === "idle"
                    ? "workflowRun.theater.notRunThisRound"
                    : state.status === "failed"
                      ? "workflowRun.conditionView.failed"
                      : state.status === "cancelled"
                        ? "workflowRun.conditionView.cancelled"
                        : "workflowRun.conditionView.pending",
              )}
        </p>
        {state.errorMessage && (
          <p className="mt-2 text-sm text-destructive">{state.errorMessage}</p>
        )}
      </section>
    </div>
  );
}
