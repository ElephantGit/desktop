import { useEffect, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import {
  UnusedMembershipContext,
  createWorkflowUnusedMembership,
} from "./execution-membership-context";

/** Carries backend-derived participation without mutating persisted node data. */
export function WorkflowMembershipProvider({
  unusedNodeIds,
  children,
}: {
  unusedNodeIds: readonly string[];
  children: ReactNode;
}) {
  const [membership] = useState(createWorkflowUnusedMembership);
  useEffect(() => {
    if (membership.replace(unusedNodeIds)) {
      membership.emit();
    }
  }, [membership, unusedNodeIds]);
  return (
    <UnusedMembershipContext.Provider value={membership}>
      {children}
    </UnusedMembershipContext.Provider>
  );
}

/** Distinguishes static exclusion from pending execution and unselected branches. */
export function WorkflowUnusedBadge() {
  const { t } = useTranslation();
  return (
    <span
      className="rounded bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground"
      title={t("workflowNode.unusedHint")}
    >
      {t("workflowNode.unused")}
    </span>
  );
}
