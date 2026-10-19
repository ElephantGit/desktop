import { beforeEach, describe, expect, it, vi } from "vitest";
import { toast } from "@ora/ui";
import type { ScheduledTask, ScheduledTaskHistoryEntry } from "@ora/contracts";
import {
  createTestClient,
  type TestHandlers,
} from "../../test/contracts-transport";
import { createTestQueryClient } from "../../test/hook-harness";
import type { PlatformAdapter } from "../../platform";
import { useUiStore } from "../stores/ui-store";
import {
  markScheduledTaskHistorySeen,
  surfaceStartupScheduledFacts,
} from "./scheduled-task-startup-surface";

const TASK: ScheduledTask = {
  id: "scheduled-1",
  name: "Daily review",
  enabled: true,
  schedule: { kind: "daily", hour: 9, minute: 30 },
  executionLocation: "local",
  projectId: "project-1",
  agentRef: "ora-space/codex",
  model: null,
  prompt: "Review the current changes",
  revision: 1n,
  approval: { policy: "full_auto_in_worktree", consentedAt: 10n },
  createdAt: 10n,
  updatedAt: 10n,
};

function missedFact(observedAt: bigint): ScheduledTaskHistoryEntry {
  return {
    kind: "missed",
    scheduledAt: 1_000n,
    reason: "app_not_running",
    observedAt,
  };
}

function failedRun(updatedAt: bigint): ScheduledTaskHistoryEntry {
  return {
    kind: "run",
    id: "run-1",
    scheduledAt: 2_000n,
    revision: 1n,
    status: "failed",
    sessionId: "session-1",
    worktreePath: null,
    worktreeTaskId: null,
    failureReason: "timeout",
    createdAt: 2_000n,
    updatedAt,
  };
}

function createPlatform(): {
  platform: PlatformAdapter;
  show: ReturnType<typeof vi.fn>;
} {
  const show = vi.fn().mockResolvedValue(true);
  return {
    show,
    platform: {
      desktopNotifications: { show },
    } as unknown as PlatformAdapter,
  };
}

function harness(entries: ScheduledTaskHistoryEntry[]) {
  const handlers: TestHandlers = {
    listScheduledTasks: async () => ({ tasks: [TASK] }),
    listScheduledTaskHistory: async () => ({ entries: [...entries] }),
  };
  return {
    client: createTestClient(handlers),
    queryClient: createTestQueryClient(),
  };
}

describe("startup surface of persisted scheduled task facts", () => {
  let errorToast: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    vi.restoreAllMocks();
    errorToast = vi.spyOn(toast, "error").mockImplementation(() => "id");
    window.localStorage.clear();
    useUiStore.setState({ scheduledTasksOpen: false });
  });

  it("surfaces a missed fact recorded while the shell was closed and advances the baseline", async () => {
    const { platform, show } = createPlatform();
    const first = harness([missedFact(5_000n)]);

    await surfaceStartupScheduledFacts(
      first.client,
      first.queryClient,
      platform,
    );

    expect(errorToast).toHaveBeenCalledTimes(1);
    expect(errorToast.mock.calls[0]?.[1]?.description).toContain(
      "已错过：Desktop 未运行",
    );
    const action = errorToast.mock.calls[0]?.[1]?.action;
    action?.onClick();
    expect(useUiStore.getState().scheduledTaskHistoryRequest).toEqual({
      taskId: TASK.id,
    });
    expect(window.localStorage.getItem("ora.scheduledTaskFactsSeenAt")).toBe(
      "5000",
    );
    expect(show).not.toHaveBeenCalled();

    // The second pass must not repeat the same fact.
    await surfaceStartupScheduledFacts(
      first.client,
      first.queryClient,
      platform,
    );
    expect(errorToast).toHaveBeenCalledTimes(1);
  });

  it("surfaces a failed run and skips facts at or before the baseline", async () => {
    window.localStorage.setItem("ora.scheduledTaskFactsSeenAt", "3000");
    const { platform } = createPlatform();
    const storage = harness([failedRun(3_000n), failedRun(4_000n)]);

    await surfaceStartupScheduledFacts(
      storage.client,
      storage.queryClient,
      platform,
    );

    expect(errorToast).toHaveBeenCalledTimes(1);
    expect(errorToast.mock.calls[0]?.[1]?.description).toContain(
      "失败：运行超时",
    );
    expect(window.localStorage.getItem("ora.scheduledTaskFactsSeenAt")).toBe(
      "4000",
    );
  });

  it("records a live fact before a reconnect can replay it from history", async () => {
    const { platform } = createPlatform();
    const storage = harness([failedRun(5_000n)]);

    await markScheduledTaskHistorySeen(
      storage.client,
      storage.queryClient,
      TASK.id,
    );
    await surfaceStartupScheduledFacts(
      storage.client,
      storage.queryClient,
      platform,
    );

    expect(errorToast).not.toHaveBeenCalled();
    expect(window.localStorage.getItem("ora.scheduledTaskFactsSeenAt")).toBe(
      "5000",
    );
  });

  it("sends the OS notification while the window is hidden and keeps the entry for refocus", async () => {
    Object.defineProperty(document, "visibilityState", {
      value: "hidden",
      configurable: true,
    });
    const { platform, show } = createPlatform();
    const storage = harness([missedFact(5_000n)]);

    await surfaceStartupScheduledFacts(
      storage.client,
      storage.queryClient,
      platform,
    );

    expect(show).toHaveBeenCalledTimes(1);
    // While hidden the toast must not fire, or it would expire unseen; the navigation
    // entry is retained and rendered when the window is visible again.
    expect(errorToast).not.toHaveBeenCalled();
    expect(window.localStorage.getItem("ora.scheduledTaskFactsSeenAt")).toBe(
      "5000",
    );

    Object.defineProperty(document, "visibilityState", {
      value: "visible",
      configurable: true,
    });
    document.dispatchEvent(new Event("visibilitychange"));
    await Promise.resolve();
    expect(errorToast).toHaveBeenCalledTimes(1);
    expect(errorToast.mock.calls[0]?.[1]?.description).toContain(
      "已错过：Desktop 未运行",
    );
  });
});
