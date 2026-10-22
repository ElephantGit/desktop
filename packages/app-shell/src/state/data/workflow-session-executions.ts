import type { GetWorkflowRunResponse } from "@ora/contracts";
import { recordedAttemptNumber, parseRetryWait } from "./workflow-run-retry";

/** An execution reference stays distinct even when several executions reuse one Ora session. */
export interface WorkflowSessionExecution {
  key: string;
  nodeId: string;
  scopeId: string;
  iteration?: number;
  loopRound?: number;
  attempt?: number;
  status: string;
  sessionId?: string;
}

/** Projects persisted rows, never the diagram's repaired or latest-round node state. */
export function workflowSessionExecutions(
  detail: Pick<GetWorkflowRunResponse, "nodes" | "scopes" | "failedAttempts">,
): WorkflowSessionExecution[] {
  const scopes = new Map(
    (detail.scopes ?? []).map((scope) => [scope.id, scope]),
  );
  const rows: WorkflowSessionExecution[] = [];
  for (const node of detail.nodes) {
    if (
      node.nodeType !== "agent" &&
      node.nodeType !== "interactive" &&
      node.sessionId === null
    )
      continue;
    let attempt: number | undefined;
    try {
      const payload = JSON.parse(node.payload ?? "{}");
      // Automatic retry counts are not execution attempt numbers after a manual resume.
      attempt =
        recordedAttemptNumber(payload?.error_detail) ??
        parseRetryWait(payload?.retry_wait)?.attempt;
    } catch {
      /* Missing attempt metadata is unknown, not an inferred first attempt. */
    }
    rows.push({
      key: node.id,
      nodeId: node.nodeId,
      scopeId: node.scopeId,
      iteration: node.iteration ?? undefined,
      loopRound: scopes.get(node.scopeId)?.roundIndex,
      attempt,
      status: node.status,
      sessionId: node.sessionId ?? undefined,
    });
  }
  for (const failure of detail.failedAttempts ?? []) {
    if (rows.some((row) => row.key === failure.nodeRunId)) continue;
    rows.push({
      key: failure.nodeRunId,
      nodeId: failure.nodeId,
      scopeId: failure.scopeId,
      iteration: failure.iteration ?? undefined,
      loopRound: scopes.get(failure.scopeId)?.roundIndex,
      attempt: failure.attempt,
      status: "failed",
      sessionId: failure.sessionId ?? undefined,
    });
  }
  return rows;
}
