import type { ScheduledTask, ScheduledTaskEvent } from "@ora/contracts";
import type { QueryClient } from "@tanstack/react-query";
import { appI18n } from "../../i18n/i18n-instance";
import type { PlatformAdapter } from "../../platform";
import { toast } from "@ora/ui";
import { scheduledTaskKeys } from "../hooks/use-scheduled-tasks";
import { useUiStore } from "../stores/ui-store";
import { useWorkspaceSelectionStore } from "../stores/workspace-selection-store";
import { invalidateSessions } from "./sessions";

/** Reads the scheduled task name from the definition cache without re-querying. */
function taskName(queryClient: QueryClient, taskId: string): string {
  const tasks =
    queryClient.getQueryData<ScheduledTask[]>(scheduledTaskKeys.all) ?? [];
  return tasks.find((task) => task.id === taskId)?.name ?? taskId;
}

/** Opens the Scheduled Task pane at one task's run history. */
function openTaskHistory(taskId: string): void {
  useUiStore.getState().openScheduledTaskHistory({ taskId });
}

/** Leaves the Scheduled Task pane and opens the run's session in its worktree. */
function openRunSession(
  sessionId: string,
  worktreeTaskId: string,
  projectId: string,
): void {
  const ui = useUiStore.getState();
  ui.setScheduledTasksOpen(false);
  ui.expandProject(projectId);
  ui.expandTask(worktreeTaskId);
  useWorkspaceSelectionStore
    .getState()
    .selectSession(sessionId, worktreeTaskId, projectId);
}

/** Shows one OS notification while the window cannot show an actionable toast. */
async function showOsNotification(
  platform: PlatformAdapter | null,
  title: string,
  body: string,
): Promise<void> {
  if (document.visibilityState === "visible") return;
  try {
    await platform?.desktopNotifications?.show({ title, body });
  } catch {
    // A notification failure never rewrites run facts; the next refresh shows history.
  }
}

/**
 * Surfaces one persisted Scheduled Task fact to the user.
 *
 * Facts arrive only after the backend committed them. When the window is visible an
 * actionable toast offers navigation; otherwise a best-effort OS notification is sent,
 * because the desktop notification plugin cannot navigate on click.
 */
export async function surfaceScheduledTaskEvent(
  platform: PlatformAdapter | null,
  event: ScheduledTaskEvent,
  queryClient: QueryClient,
): Promise<void> {
  queryClient.invalidateQueries({
    queryKey: scheduledTaskKeys.history(event.taskId),
  });
  const t = appI18n.t.bind(appI18n);
  const name = taskName(queryClient, event.taskId);

  if (event.kind === "run_started") {
    await showOsNotification(
      platform,
      t("scheduledTask.notificationStartedTitle"),
      t("scheduledTask.notificationStartedBody", { name }),
    );
    invalidateSessions(queryClient);
    toast.success(t("scheduledTask.notificationStartedTitle"), {
      description: t("scheduledTask.notificationStartedBody", { name }),
      action: {
        label: t("scheduledTask.view"),
        onClick: () =>
          openRunSession(
            event.sessionId,
            event.worktreeTaskId,
            event.projectId,
          ),
      },
    });
    return;
  }

  if (event.kind === "run_finished") {
    if (event.status !== "failed") {
      // The ADR notifies starts, failures, and misses; successes stay in history.
      return;
    }
    const reason = event.failureReason
      ? t(`scheduledTask.failureReason.${event.failureReason}`)
      : t("scheduledTask.failureReason.execution_error");
    await showOsNotification(
      platform,
      t("scheduledTask.notificationFailedTitle"),
      `${name} · ${reason}`,
    );
    toast.error(t("scheduledTask.notificationFailedTitle"), {
      description: `${name} · ${reason}`,
      action: {
        label: t("scheduledTask.view"),
        onClick: () => openTaskHistory(event.taskId),
      },
    });
    return;
  }

  const reason = t(`scheduledTask.historyReason.${event.reason}`);
  await showOsNotification(
    platform,
    t("scheduledTask.notificationMissedTitle"),
    `${name} · ${reason}`,
  );
  toast.error(t("scheduledTask.notificationMissedTitle"), {
    description: `${name} · ${reason}`,
    action: {
      label: t("scheduledTask.view"),
      onClick: () => openTaskHistory(event.taskId),
    },
  });
}
