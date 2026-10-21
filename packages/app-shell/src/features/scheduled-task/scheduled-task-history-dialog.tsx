import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import type { ScheduledTask, ScheduledTaskHistoryEntry } from "@ora/contracts";
import {
  Button,
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  Spinner,
  toast,
} from "@ora/ui";
import { localizeContractError } from "../../i18n/contract-error";
import {
  scheduledTaskKeys,
  useScheduledTaskHistory,
} from "../../state/hooks/use-scheduled-tasks";
import { useTasks } from "../../state/hooks/use-tasks";
import { useDeleteTask } from "../../state/hooks/use-workspace-mutations";
import { useUiStore } from "../../state/stores/ui-store";
import { useWorkspaceSelectionStore } from "../../state/stores/workspace-selection-store";
import { ScheduledTaskConfirmDialog } from "./scheduled-task-confirm-dialog";
import {
  formatTimestamp,
  historyStatusLabel,
  historyVerdictLabel,
} from "./scheduled-task-format";

/** The run statuses whose history entry may still reference a retained worktree. */
const RETAINED_WORKTREE_STATUSES = new Set([
  "succeeded",
  "failed",
  "cancelled",
]);

/** Shows the newest-first persisted run and missed facts for one task. */
export function ScheduledTaskHistoryDialog({
  task,
  onOpenChange,
}: {
  task: ScheduledTask;
  onOpenChange(open: boolean): void;
}) {
  const { t } = useTranslation();
  const history = useScheduledTaskHistory(task.id);
  const tasksQuery = useTasks();
  const queryClient = useQueryClient();
  const deleteWorktreeTask = useDeleteTask();
  const [discardTaskId, setDiscardTaskId] = useState<string | null>(null);

  const close = () => onOpenChange(false);

  const openWorktree = (
    entry: Extract<ScheduledTaskHistoryEntry, { kind: "run" }>,
  ) => {
    if (entry.worktreeTaskId === null) return;
    // The worktree task carries the project the run actually executed in; the definition's
    // project is editable and later runs may target a different one.
    const worktreeProjectId =
      (tasksQuery.data ?? []).find(
        (worktreeTask) => worktreeTask.id === entry.worktreeTaskId,
      )?.projectId ?? task.projectId;
    const ui = useUiStore.getState();
    ui.setScheduledTasksOpen(false);
    ui.expandProject(worktreeProjectId);
    ui.expandTask(entry.worktreeTaskId);
    const selection = useWorkspaceSelectionStore.getState();
    if (entry.sessionId !== null) {
      selection.selectSession(
        entry.sessionId,
        entry.worktreeTaskId,
        worktreeProjectId,
      );
    } else {
      selection.selectTask(entry.worktreeTaskId, worktreeProjectId);
    }
    close();
  };

  const discardWorktree = (worktreeTaskId: string) => {
    deleteWorktreeTask.mutate(
      { taskId: worktreeTaskId },
      {
        onSuccess: () => {
          toast.success(t("scheduledTask.discarded"));
          void queryClient.invalidateQueries({
            queryKey: scheduledTaskKeys.history(task.id),
          });
        },
        onError: (error) => {
          toast.error(t("scheduledTask.discardTitle"), {
            description: localizeContractError(error, t),
          });
        },
        onSettled: () => setDiscardTaskId(null),
      },
    );
  };

  const worktreeTaskIds = new Set(
    (tasksQuery.data ?? []).map((worktreeTask) => worktreeTask.id),
  );

  return (
    <Dialog open onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[80vh] overflow-y-auto sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>{t("scheduledTask.history")}</DialogTitle>
          <DialogDescription>{task.name}</DialogDescription>
        </DialogHeader>
        {history.isPending && (
          <div className="flex items-center gap-2 text-sm text-muted-foreground">
            <Spinner className="size-4" />
            {t("scheduledTask.historyLoading")}
          </div>
        )}
        {history.error && (
          <p data-selectable className="text-sm text-destructive">
            {localizeContractError(history.error, t)}
          </p>
        )}
        {!history.isPending && !history.error && history.data?.length === 0 && (
          <p className="rounded-lg border border-dashed p-6 text-center text-sm text-muted-foreground">
            {t("scheduledTask.historyEmpty")}
          </p>
        )}
        <div className="grid gap-2">
          {history.data?.map((entry) => (
            <article
              key={
                entry.kind === "run"
                  ? entry.id
                  : `missed-${entry.scheduledAt.toString()}`
              }
              className="rounded-lg border p-3"
            >
              <div className="flex items-center justify-between gap-3">
                <span className="text-sm font-medium">
                  {historyStatusLabel(entry, t)}
                </span>
                <span className="text-xs text-muted-foreground">
                  {formatTimestamp(entry.scheduledAt)}
                </span>
              </div>
              {entry.kind === "run" && (
                <>
                  <p className="mt-1 text-xs text-muted-foreground">
                    {t("scheduledTask.historyRevision", {
                      revision: entry.revision.toString(),
                    })}
                    {entry.sessionId === null
                      ? ""
                      : ` · ${t("scheduledTask.historySession", {
                          session: entry.sessionId,
                        })}`}
                    {entry.worktreePath === null
                      ? ""
                      : ` · ${t("scheduledTask.historyWorktree", {
                          worktree: entry.worktreePath,
                        })}`}
                  </p>
                  {historyVerdictLabel(entry, t) !== null && (
                    <p className="mt-1 text-xs">
                      {t("scheduledTask.verdictLabel")}:{" "}
                      {historyVerdictLabel(entry, t)}
                    </p>
                  )}
                  {entry.worktreeTaskId !== null &&
                    RETAINED_WORKTREE_STATUSES.has(entry.status) &&
                    worktreeTaskIds.has(entry.worktreeTaskId) && (
                      <div className="mt-2 flex justify-end gap-2">
                        <Button
                          type="button"
                          size="sm"
                          variant="ghost"
                          onClick={() => openWorktree(entry)}
                        >
                          {t("scheduledTask.open")}
                        </Button>
                        <Button
                          type="button"
                          size="sm"
                          variant="ghost"
                          disabled={deleteWorktreeTask.isPending}
                          onClick={() => setDiscardTaskId(entry.worktreeTaskId)}
                        >
                          {t("scheduledTask.discard")}
                        </Button>
                      </div>
                    )}
                </>
              )}
            </article>
          ))}
        </div>
        {discardTaskId !== null && (
          <ScheduledTaskConfirmDialog
            title={t("scheduledTask.discardTitle")}
            description={t("scheduledTask.discardConfirm")}
            confirmLabel={t("scheduledTask.discard")}
            busy={deleteWorktreeTask.isPending}
            onConfirm={() => discardWorktree(discardTaskId)}
            onOpenChange={(open) => !open && setDiscardTaskId(null)}
          />
        )}
      </DialogContent>
    </Dialog>
  );
}
