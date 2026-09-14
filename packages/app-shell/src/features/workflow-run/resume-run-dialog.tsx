import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import type { ResumeRollbackMode } from "@ora/contracts";
import {
  AlertDialog,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  Button,
  RadioGroup,
  RadioGroupItem,
  Spinner,
} from "@ora/ui";
import { localizeContractError } from "../../i18n/contract-error";
import {
  usePreviewWorkflowRunResume,
  useResumeWorkflowRun,
} from "../../state/data/workflow-runs";

interface ResumeRunDialogProps {
  open: boolean;
  runId: string;
  onOpenChange: (open: boolean) => void;
  onResumed: () => void;
}

const REASON_KEYS = {
  no_checkpoint: "workflowRun.resume.reason.no_checkpoint",
  siblings_ran_after_checkpoint:
    "workflowRun.resume.reason.siblings_ran_after_checkpoint",
  not_resumable: "workflowRun.resume.reason.not_resumable",
} as const;

/** Confirms how to treat the worktree before resuming a failed or cancelled run. */
export function ResumeRunDialog({
  open,
  runId,
  onOpenChange,
  onResumed,
}: ResumeRunDialogProps) {
  const { t } = useTranslation();
  const preview = usePreviewWorkflowRunResume();
  const resume = useResumeWorkflowRun();
  const [rollback, setRollback] = useState<ResumeRollbackMode>("keep");
  const [error, setError] = useState<string | null>(null);
  const [seedKey, setSeedKey] = useState<string | null>(null);
  const previewMutate = preview.mutate;
  const previewReset = preview.reset;
  const nextSeedKey = open ? runId : null;
  if (nextSeedKey !== null && nextSeedKey !== seedKey) {
    setSeedKey(nextSeedKey);
    setRollback("keep");
    setError(null);
  }
  if (!open && seedKey !== null) {
    setSeedKey(null);
  }

  useEffect(() => {
    if (!open) {
      return;
    }
    previewReset();
    previewMutate({ runId });
  }, [open, runId, previewMutate, previewReset]);

  const previewData = preview.data;
  const checkpointReason = checkpointReasonText(
    previewData?.checkpointUnavailableReason,
    t,
  );

  /** Submits the chosen rollback mode and closes only after the run actually resumes. */
  async function confirm(): Promise<void> {
    setError(null);
    try {
      await resume.mutateAsync({ runId, rollback });
      onResumed();
      onOpenChange(false);
    } catch (cause) {
      setError(localizeContractError(cause, t));
    }
  }

  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogContent className="sm:max-w-lg">
        <AlertDialogHeader>
          <AlertDialogTitle>{t("workflowRun.resume.title")}</AlertDialogTitle>
          <AlertDialogDescription>
            {t("workflowRun.resume.description")}
          </AlertDialogDescription>
        </AlertDialogHeader>

        {preview.isPending ? (
          <p className="mt-2 text-xs text-muted-foreground" role="status">
            {t("workflowRun.resume.loadingPreview")}
          </p>
        ) : null}
        {preview.isError ? (
          <p className="mt-2 text-xs text-destructive" role="alert">
            {t("workflowRun.resume.previewFailed")}
          </p>
        ) : null}

        {previewData
          ? previewData.failedNodes.map((node) => {
              const recorded = new Set(
                node.nodeFileChanges.map((change) => change.path),
              );
              const extra = node.changedSinceCheckpoint.filter(
                (change) => !recorded.has(change.path),
              ).length;
              const paths = [
                ...node.nodeFileChanges.map((change) => change.path),
                ...node.changedSinceCheckpoint
                  .map((change) => change.path)
                  .filter((path) => !recorded.has(path)),
              ];
              return (
                <div key={node.nodeRunId} className="mt-2 space-y-1.5">
                  <p className="text-xs leading-5">
                    {t("workflowRun.resume.nodeSummary", {
                      nodeId: node.nodeId,
                      nodeFiles: node.nodeFileChanges.length,
                      total: node.changedSinceCheckpoint.length,
                      extra,
                    })}
                  </p>
                  {paths.length > 0 ? (
                    <details>
                      <summary className="cursor-pointer text-xs text-muted-foreground">
                        {t("workflowRun.field.fileChanges")}
                      </summary>
                      <ul className="mt-1 max-h-24 overflow-auto text-xs">
                        {paths.map((path) => (
                          <li key={path}>{path}</li>
                        ))}
                      </ul>
                    </details>
                  ) : null}
                </div>
              );
            })
          : null}

        <RadioGroup
          className="mt-3 gap-2"
          value={rollback}
          onValueChange={(value) => {
            if (
              value === "keep" ||
              value === "node_files" ||
              value === "checkpoint"
            ) {
              setRollback(value);
            }
          }}
        >
          <label className="flex items-start gap-2 text-sm">
            <RadioGroupItem value="keep" className="mt-0.5" />
            <span>{t("workflowRun.resume.keep")}</span>
          </label>
          <label
            className={`flex items-start gap-2 text-sm ${
              previewData && !previewData.nodeFilesAvailable
                ? "opacity-50"
                : ""
            }`}
          >
            <RadioGroupItem
              value="node_files"
              className="mt-0.5"
              disabled={previewData !== undefined && !previewData.nodeFilesAvailable}
            />
            <span>{t("workflowRun.resume.nodeFiles")}</span>
          </label>
          <label
            className={`flex items-start gap-2 text-sm ${
              previewData && !previewData.checkpointAvailable
                ? "opacity-50"
                : ""
            }`}
          >
            <RadioGroupItem
              value="checkpoint"
              className="mt-0.5"
              disabled={
                previewData !== undefined && !previewData.checkpointAvailable
              }
            />
            <span className="space-y-1">
              <span className="block">{t("workflowRun.resume.checkpoint")}</span>
              {checkpointReason ? (
                <span className="block text-xs text-muted-foreground">
                  {checkpointReason}
                </span>
              ) : null}
            </span>
          </label>
        </RadioGroup>

        <p className="mt-2 text-xs text-muted-foreground">
          {t("workflowRun.resume.safetyNote")}
        </p>

        {error ? (
          <p className="mt-2 text-xs text-destructive" role="alert">
            {error}
          </p>
        ) : null}

        <AlertDialogFooter>
          <AlertDialogCancel>{t("common.cancel")}</AlertDialogCancel>
          <Button
            type="button"
            disabled={
              preview.isPending ||
              preview.isError ||
              previewData === undefined ||
              resume.isPending
            }
            onClick={() => void confirm()}
          >
            {resume.isPending ? (
              <span className="inline-flex items-center gap-1.5">
                <Spinner className="size-3.5" />
                {t("workflowRun.resume.confirm")}
              </span>
            ) : (
              t("workflowRun.resume.confirm")
            )}
          </Button>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

/** Maps a backend availability reason onto the matching translated explanation. */
function checkpointReasonText(
  reason: string | null | undefined,
  t: TFunction,
): string | null {
  if (reason === null || reason === undefined) {
    return null;
  }
  if (reason === "no_checkpoint") {
    return t(REASON_KEYS.no_checkpoint);
  }
  if (reason === "siblings_ran_after_checkpoint") {
    return t(REASON_KEYS.siblings_ran_after_checkpoint);
  }
  if (reason === "not_resumable") {
    return t(REASON_KEYS.not_resumable);
  }
  return null;
}
