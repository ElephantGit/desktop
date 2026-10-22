import { describe, expect, it } from "vitest";
import type {
  WorkflowNodeRun,
  WorkflowNodeFailedAttempt,
} from "@ora/contracts";
import { workflowSessionExecutions } from "./workflow-session-executions";

const node = (
  id: string,
  sessionId: string | null,
  extra: Partial<WorkflowNodeRun> = {},
): WorkflowNodeRun => ({
  id,
  nodeId: id,
  runId: "run",
  scopeId: "root",
  nodeType: "agent",
  sessionId,
  status: "succeeded",
  input: null,
  output: null,
  error: null,
  payload: null,
  iteration: null,
  startedAt: 1n,
  finishedAt: 2n,
  createdAt: 0n,
  updatedAt: 2n,
  ...extra,
});
const failure: WorkflowNodeFailedAttempt = {
  nodeRunId: "a-first",
  nodeId: "a",
  scopeId: "root",
  sessionId: "first",
  iteration: null,
  attempt: 1,
  kind: "provider",
  message: "failed",
  sourceChain: [],
  startedAt: 1n,
  finishedAt: 2n,
  recordedAt: 2n,
};

describe("persisted workflow session executions", () => {
  it("keeps three executions for two nodes and a failed attempt, even when ids are shared", () => {
    const rows = workflowSessionExecutions({
      nodes: [node("a", "shared"), node("b", "shared")],
      failedAttempts: [failure],
    });
    expect(
      rows.map(({ key, sessionId, status, attempt }) => ({
        key,
        sessionId,
        status,
        attempt,
      })),
    ).toEqual([
      {
        key: "a",
        sessionId: "shared",
        status: "succeeded",
        attempt: undefined,
      },
      {
        key: "b",
        sessionId: "shared",
        status: "succeeded",
        attempt: undefined,
      },
      { key: "a-first", sessionId: "first", status: "failed", attempt: 1 },
    ]);
  });
  it("retains scope, raw round and no-session failures without deriving attempts from retry count", () => {
    const rows = workflowSessionExecutions({
      nodes: [
        node("a", null, {
          scopeId: "loop",
          iteration: 0,
          status: "failed",
          payload: '{"auto_retry":{"retry":3,"max_retries":4}}',
        }),
      ],
      scopes: [
        {
          id: "loop",
          runId: "run",
          parentLoopNodeRunId: "parent",
          roundIndex: 2,
          status: "failed",
          createdAt: 0n,
          updatedAt: 1n,
        },
      ],
    });
    expect(rows).toEqual([
      {
        key: "a",
        nodeId: "a",
        scopeId: "loop",
        iteration: 0,
        loopRound: 2,
        attempt: undefined,
        status: "failed",
        sessionId: undefined,
      },
    ]);
  });
});
