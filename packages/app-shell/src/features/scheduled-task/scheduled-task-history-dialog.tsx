import { useTranslation } from "react-i18next";
import type { ScheduledTask } from "@ora/contracts";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  Spinner,
} from "@ora/ui";
import { localizeContractError } from "../../i18n/contract-error";
import { useScheduledTaskHistory } from "../../state/hooks/use-scheduled-tasks";
import { formatTimestamp, historyStatusLabel } from "./scheduled-task-format";

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
              )}
            </article>
          ))}
        </div>
      </DialogContent>
    </Dialog>
  );
}
