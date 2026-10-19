import { beforeEach, describe, expect, it, vi } from "vitest";
import { toast } from "@ora/ui";
import type { ScheduledTask, ScheduledTaskEvent } from "@ora/contracts";
import { createTestQueryClient } from "../../test/hook-harness";
import type { PlatformAdapter } from "../../platform";
import { scheduledTaskKeys } from "../hooks/use-scheduled-tasks";
import { useUiStore } from "../stores/ui-store";
import { useWorkspaceSelectionStore } from "../stores/workspace-selection-store";
import { surfaceScheduledTaskEvent } from "./scheduled-task-events";

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

async function surface(
  platform: PlatformAdapter | null,
  event: ScheduledTaskEvent,
): Promise<void> {
  const queryClient = createTestQueryClient();
  queryClient.setQueryData(scheduledTaskKeys.all, [TASK]);
  await surfaceScheduledTaskEvent(platform, event, queryClient);
}

function setVisibility(state: DocumentVisibilityState): void {
  Object.defineProperty(document, "visibilityState", {
    value: state,
    configurable: true,
  });
}

describe("scheduled task event surfacing", () => {
  let successToast: ReturnType<typeof vi.spyOn>;
  let errorToast: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    vi.restoreAllMocks();
    successToast = vi.spyOn(toast, "success").mockImplementation(() => "id");
    errorToast = vi.spyOn(toast, "error").mockImplementation(() => "id");
    setVisibility("visible");
    useUiStore.setState({ scheduledTasksOpen: false });
    useWorkspaceSelectionStore.getState().clearSelection();
  });

  it("announces a started run with a toast that navigates to the new session", async () => {
    const { platform, show } = createPlatform();

    await surface(platform, {
      kind: "run_started",
      taskId: "scheduled-1",
      sessionId: "session-1",
      worktreeTaskId: "worktree-task-1",
      projectId: "project-1",
    });

    expect(successToast).toHaveBeenCalledTimes(1);
    const action = successToast.mock.calls[0]?.[1]?.action;
    expect(action).toBeDefined();
    action?.onClick();
    const selection = useWorkspaceSelectionStore.getState().selection;
    expect(selection).toMatchObject({
      sessionId: "session-1",
      taskId: "worktree-task-1",
      projectId: "project-1",
    });
    expect(useUiStore.getState().scheduledTasksOpen).toBe(false);
    expect(show).not.toHaveBeenCalled();
  });

  it("sends the OS notification for a started run only while the window is hidden", async () => {
    setVisibility("hidden");
    const { platform, show } = createPlatform();

    await surface(platform, {
      kind: "run_started",
      taskId: "scheduled-1",
      sessionId: "session-1",
      worktreeTaskId: "worktree-task-1",
      projectId: "project-1",
    });

    expect(show).toHaveBeenCalledTimes(1);
    expect(show.mock.calls[0]?.[0]?.title).toContain("定时任务已开始");
  });

  it("announces a failed run without notifying successful or cancelled runs", async () => {
    const { platform, show } = createPlatform();

    await surface(platform, {
      kind: "run_finished",
      taskId: "scheduled-1",
      status: "failed",
      failureReason: "timeout",
    });
    expect(errorToast).toHaveBeenCalledTimes(1);
    expect(errorToast.mock.calls[0]?.[1]?.description).toContain(
      "失败：运行超时",
    );

    await surface(platform, {
      kind: "run_finished",
      taskId: "scheduled-1",
      status: "succeeded",
      failureReason: null,
    });
    expect(successToast).not.toHaveBeenCalled();
    expect(errorToast).toHaveBeenCalledTimes(1);
    expect(show).not.toHaveBeenCalled();
  });

  it("announces a missed occurrence and deep-links to the task history", async () => {
    const { platform, show } = createPlatform();

    await surface(platform, {
      kind: "occurrence_missed",
      taskId: "scheduled-1",
      scheduledAt: 1_000n,
      reason: "app_not_running",
    });

    expect(errorToast).toHaveBeenCalledTimes(1);
    const action = errorToast.mock.calls[0]?.[1]?.action;
    action?.onClick();
    expect(useUiStore.getState().scheduledTasksOpen).toBe(true);
    expect(useUiStore.getState().scheduledTaskHistoryRequest).toEqual({
      taskId: "scheduled-1",
    });
    expect(show).not.toHaveBeenCalled();
  });
});
