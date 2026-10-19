import type {
  ContractsClient,
  ScheduledTask,
  ScheduledTaskEvent,
} from "@ora/contracts";
import type { QueryClient } from "@tanstack/react-query";
import { appI18n } from "../../i18n/i18n-instance";
import type { PlatformAdapter } from "../../platform";
import { toast } from "@ora/ui";
import { scheduledTaskKeys } from "../hooks/use-scheduled-tasks";
import { useUiStore } from "../stores/ui-store";
import { useWorkspaceSelectionStore } from "../stores/workspace-selection-store";
import { invalidateSessions } from "./sessions";

/** Reads the scheduled task name from the definition cache without re-querying. */
async function taskName(
  queryClient: QueryClient,
  client: ContractsClient,
  taskId: string,
): Promise<string> {
  const tasks =
    queryClient.getQueryData<ScheduledTask[]>(scheduledTaskKeys.all) ??
    (await queryClient
      .fetchQuery({
        queryKey: scheduledTaskKeys.all,
        queryFn: () => client.scheduledTask.list({}).then((r) => r.tasks),
      })
      .catch(() => []));
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

/** One fact whose navigation entry waits for the window to become visible again. */
interface PendingFact {
  variant: "success" | "error";
  title: string;
  body: string;
  onClick?: () => void;
}

/**
 * Facts notified while the window was hidden. The OS notification cannot navigate on
 * desktop, so each one keeps its in-app entry here until the window is visible again;
 * without this queue a hidden-window notification would lose its navigation path.
 */
const pendingFacts: PendingFact[] = [];
const MAX_PENDING_FACTS = 20;
let focusFlushRegistered = false;

function flushPendingFacts(): void {
  while (pendingFacts.length > 0) {
    const fact = pendingFacts.shift()!;
    const action = fact.onClick
      ? { label: appI18n.t("scheduledTask.view"), onClick: fact.onClick }
      : undefined;
    if (fact.variant === "success") {
      toast.success(fact.title, { description: fact.body, action });
    } else {
      toast.error(fact.title, { description: fact.body, action });
    }
  }
}

function registerFocusFlush(): void {
  if (focusFlushRegistered || typeof document === "undefined") return;
  focusFlushRegistered = true;
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "visible") flushPendingFacts();
  });
}

/**
 * Sends one OS notification while the window cannot show a toast, and retains the fact's
 * navigation entry until the window becomes visible again.
 */
async function showOsNotification(
  platform: PlatformAdapter | null,
  fact: PendingFact,
): Promise<void> {
  try {
    await platform?.desktopNotifications?.show({
      title: fact.title,
      body: fact.body,
    });
  } catch {
    // A notification failure never rewrites run facts; the next refresh shows history.
  }
  pendingFacts.push(fact);
  if (pendingFacts.length > MAX_PENDING_FACTS) pendingFacts.shift();
  registerFocusFlush();
}

/** Renders one failed-or-missed fact as an actionable in-app toast right now. */
function toastFailedFact(title: string, body: string, onClick?: () => void): void {
  toast.error(title, {
    description: body,
    action: onClick
      ? { label: appI18n.t("scheduledTask.view"), onClick }
      : undefined,
  });
}

/**
 * Surfaces one failed or missed Scheduled Task fact: an actionable in-app toast while the
 * window is visible; while it is hidden, a best-effort OS notification plus a retained
 * entry that renders the same toast when the window is visible again.
 */
export async function notifyFailedScheduledFact(
  platform: PlatformAdapter | null,
  title: string,
  body: string,
  onClick?: () => void,
): Promise<void> {
  if (document.visibilityState === "visible") {
    toastFailedFact(title, body, onClick);
    return;
  }
  await showOsNotification(platform, {
    variant: "error",
    title,
    body,
    onClick,
  });
}

/**
 * Surfaces one persisted Scheduled Task fact to the user.
 *
 * Facts arrive only after the backend committed them. While the window is visible an
 * actionable toast offers navigation; while it is hidden a best-effort OS notification is
 * sent — the desktop notification plugin cannot navigate on click — and the navigation
 * entry is retained until the window is visible again.
 */
export async function surfaceScheduledTaskEvent(
  platform: PlatformAdapter | null,
  event: ScheduledTaskEvent,
  queryClient: QueryClient,
  client: ContractsClient,
): Promise<void> {
  queryClient.invalidateQueries({
    queryKey: scheduledTaskKeys.history(event.taskId),
  });
  const t = appI18n.t.bind(appI18n);
  const name = await taskName(queryClient, client, event.taskId);

  if (event.kind === "run_started") {
    invalidateSessions(queryClient);
    const title = t("scheduledTask.notificationStartedTitle");
    const body = t("scheduledTask.notificationStartedBody", { name });
    const onClick = () =>
      openRunSession(event.sessionId, event.worktreeTaskId, event.projectId);
    if (document.visibilityState === "visible") {
      toast.success(title, { description: body, action: { label: t("scheduledTask.view"), onClick } });
      return;
    }
    await showOsNotification(platform, { variant: "success", title, body, onClick });
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
    await notifyFailedScheduledFact(
      platform,
      t("scheduledTask.notificationFailedTitle"),
      `${name} · ${reason}`,
      () => openTaskHistory(event.taskId),
    );
    return;
  }

  const reason = t(`scheduledTask.historyReason.${event.reason}`);
  await notifyFailedScheduledFact(
    platform,
    t("scheduledTask.notificationMissedTitle"),
    `${name} · ${reason}`,
    () => openTaskHistory(event.taskId),
  );
}
