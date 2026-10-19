import type {
  ContractsClient,
  ScheduledTask,
  ScheduledTaskHistoryEntry,
} from "@ora/contracts";
import type { QueryClient } from "@tanstack/react-query";
import { appI18n } from "../../i18n/i18n-instance";
import type { PlatformAdapter } from "../../platform";
import { scheduledTaskKeys } from "../hooks/use-scheduled-tasks";
import { useUiStore } from "../stores/ui-store";
import { notifyFailedScheduledFact } from "./scheduled-task-events";

const SEEN_STORAGE_KEY = "ora.scheduledTaskFactsSeenAt";

/** Reads the timestamp up to which persisted facts were already surfaced to the user. */
function readSeenBaseline(): number {
  try {
    const raw = window.localStorage.getItem(SEEN_STORAGE_KEY);
    if (raw === null) return 0;
    const value = Number(raw);
    return Number.isFinite(value) ? value : 0;
  } catch {
    return 0;
  }
}

/** Persists the surfaced timestamp; a lost write can only repeat a notification. */
function writeSeenBaseline(at: number): void {
  try {
    window.localStorage.setItem(SEEN_STORAGE_KEY, String(at));
  } catch {
    // Best effort: surfacing must not depend on storage availability.
  }
}

function asMillis(value: bigint): number {
  return Number(value);
}

/** Reports whether the entry is a durable failure or missed fact the user must hear about. */
function surfaceableFactAt(entry: ScheduledTaskHistoryEntry): number | null {
  if (entry.kind === "missed") return asMillis(entry.observedAt);
  if (entry.failureReason !== null) return asMillis(entry.updatedAt);
  return null;
}

/**
 * Surfaces persisted Scheduled Task facts that were recorded while the shell could not
 * receive application events: startup-recovered missed occurrences and interrupted failed
 * runs. Facts newer than the persisted baseline are surfaced once, then the baseline
 * advances; notifications stay best-effort and never rewrite run facts.
 */
export async function surfaceStartupScheduledFacts(
  client: ContractsClient,
  queryClient: QueryClient,
  platform: PlatformAdapter | null,
): Promise<void> {
  const t = appI18n.t.bind(appI18n);
  const baseline = readSeenBaseline();
  let newest = baseline;

  let tasks: ScheduledTask[];
  try {
    tasks = await queryClient.fetchQuery({
      queryKey: scheduledTaskKeys.all,
      queryFn: () => client.scheduledTask.list({}).then((r) => r.tasks),
    });
  } catch {
    return;
  }

  for (const task of tasks) {
    let entries: ScheduledTaskHistoryEntry[];
    try {
      entries = await queryClient.fetchQuery({
        queryKey: scheduledTaskKeys.history(task.id),
        queryFn: () =>
          client.scheduledTask
            .listHistory({ taskId: task.id })
            .then((r) => r.entries),
      });
    } catch {
      continue;
    }
    for (const entry of entries) {
      const factAt = surfaceableFactAt(entry);
      if (factAt === null || factAt <= baseline) continue;
      newest = Math.max(newest, factAt);
      const title =
        entry.kind === "missed"
          ? t("scheduledTask.notificationMissedTitle")
          : t("scheduledTask.notificationFailedTitle");
      const reason =
        entry.kind === "missed"
          ? t(`scheduledTask.historyReason.${entry.reason}`)
          : t(`scheduledTask.failureReason.${entry.failureReason}`);
      await notifyFailedScheduledFact(
        platform,
        title,
        `${task.name} · ${reason}`,
        () =>
          useUiStore.getState().openScheduledTaskHistory({ taskId: task.id }),
      );
    }
  }

  if (newest > baseline) writeSeenBaseline(newest);
}
