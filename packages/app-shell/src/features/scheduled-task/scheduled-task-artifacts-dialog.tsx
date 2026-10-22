import { useState } from "react";
import { useTranslation } from "react-i18next";
import type {
  ScheduledRunArtifactInfo,
  ScheduledRunArtifactRejectionInfo,
} from "@ora/contracts";
import {
  Button,
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  Spinner,
} from "@ora/ui";
import { localizeContractError } from "../../i18n/contract-error";
import { useScheduledArtifactDownload } from "../../state/hooks/use-scheduled-artifact-download";
import {
  useScheduledRunArtifactPreview,
  useScheduledRunArtifacts,
} from "../../state/hooks/use-scheduled-tasks";

/** Formats one artifact byte count for the history list. */
function formatArtifactSize(bytes: bigint): string {
  const value = Number(bytes);
  if (value < 1024) return `${value.toString()} B`;
  if (value < 1024 * 1024) {
    return `${(value / 1024).toFixed(1)} KiB`;
  }
  return `${(value / (1024 * 1024)).toFixed(1)} MiB`;
}

/** Lists one run's persisted artifacts and offers preview and native export. */
export function ScheduledTaskArtifactsDialog({
  runId,
  onOpenChange,
}: {
  runId: string;
  onOpenChange(open: boolean): void;
}) {
  const { t } = useTranslation();
  const artifacts = useScheduledRunArtifacts(runId, true);
  const download = useScheduledArtifactDownload();
  const [previewPath, setPreviewPath] = useState<string | null>(null);
  const preview = useScheduledRunArtifactPreview(runId, previewPath);

  return (
    <Dialog open onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[80vh] overflow-y-auto sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>{t("scheduledTask.artifacts")}</DialogTitle>
          <DialogDescription>{runId}</DialogDescription>
        </DialogHeader>
        {artifacts.isPending && (
          <div className="flex items-center gap-2 text-sm text-muted-foreground">
            <Spinner className="size-4" />
            {t("scheduledTask.artifactsLoading")}
          </div>
        )}
        {artifacts.error && (
          <p data-selectable className="text-sm text-destructive">
            {localizeContractError(artifacts.error, t)}
          </p>
        )}
        {!artifacts.isPending &&
          !artifacts.error &&
          artifacts.data?.artifacts.length === 0 && (
            <p className="rounded-lg border border-dashed p-6 text-center text-sm text-muted-foreground">
              {t("scheduledTask.artifactsEmpty")}
            </p>
          )}
        {artifacts.data !== undefined &&
          artifacts.data.artifacts.length > 0 && (
            <ul className="grid gap-2">
              {artifacts.data.artifacts.map((artifact) => (
                <ArtifactRow
                  key={artifact.relativePath}
                  runId={runId}
                  artifact={artifact}
                  download={download}
                  onPreview={() => setPreviewPath(artifact.relativePath)}
                />
              ))}
            </ul>
          )}
        {artifacts.data !== undefined &&
          artifacts.data.rejections.length > 0 && (
            <section className="mt-2">
              <p className="text-xs font-medium text-muted-foreground">
                {t("scheduledTask.artifactRejections", {
                  count: artifacts.data.rejections.length,
                })}
              </p>
              <ul className="mt-1 grid gap-1">
                {artifacts.data.rejections.map((rejection) => (
                  <RejectionRow key={rejection.name} rejection={rejection} />
                ))}
              </ul>
            </section>
          )}
        {previewPath !== null && (
          <section className="mt-3 border-t pt-3">
            <div className="flex items-center justify-between gap-2">
              <p className="min-w-0 truncate text-sm font-medium">
                {previewPath}
              </p>
              <Button
                type="button"
                size="sm"
                variant="ghost"
                onClick={() => setPreviewPath(null)}
              >
                {t("scheduledTask.artifactPreviewClose")}
              </Button>
            </div>
            {preview.isPending && (
              <div className="mt-2 flex items-center gap-2 text-sm text-muted-foreground">
                <Spinner className="size-4" />
                {t("scheduledTask.artifactsLoading")}
              </div>
            )}
            {preview.error && (
              <p data-selectable className="mt-2 text-sm text-destructive">
                {localizeContractError(preview.error, t)}
              </p>
            )}
            {preview.data != null && (
              <>
                {preview.data.truncated && (
                  <p className="mt-2 text-xs text-muted-foreground">
                    {t("scheduledTask.artifactPreviewTruncated")}
                  </p>
                )}
                <pre className="mt-2 max-h-80 overflow-auto rounded-lg border bg-muted/40 p-3 text-xs whitespace-pre-wrap">
                  {preview.data.content}
                </pre>
              </>
            )}
          </section>
        )}
      </DialogContent>
    </Dialog>
  );
}

/** Shows one artifact's name, size, and actions. */
function ArtifactRow({
  runId,
  artifact,
  download,
  onPreview,
}: {
  runId: string;
  artifact: ScheduledRunArtifactInfo;
  download: ReturnType<typeof useScheduledArtifactDownload>;
  onPreview(): void;
}) {
  const { t } = useTranslation();
  return (
    <li className="flex items-center justify-between gap-3 rounded-lg border p-3">
      <div className="min-w-0">
        <p className="truncate text-sm font-medium">{artifact.relativePath}</p>
        <p className="text-xs text-muted-foreground">
          {formatArtifactSize(artifact.sizeBytes)}
        </p>
      </div>
      <div className="flex shrink-0 gap-1">
        <Button type="button" size="sm" variant="ghost" onClick={onPreview}>
          {t("scheduledTask.artifactPreview")}
        </Button>
        {download !== undefined && (
          <Button
            type="button"
            size="sm"
            variant="ghost"
            disabled={download.isDownloading}
            onClick={() => void download.download(runId, artifact)}
          >
            {t("scheduledTask.artifactDownload")}
          </Button>
        )}
      </div>
    </li>
  );
}

/** Maps the wire reason to its translation suffix without plural-like token endings. */
const REJECTION_TRANSLATION_KEYS: Record<string, string> = {
  unsupported_type: "unsupportedType",
  path_unsafe: "pathUnsafe",
  symlink: "symlinkCase",
  hard_link: "hardLink",
  too_large: "tooLarge",
  too_many: "tooManyCount",
  total_too_large: "totalTooLarge",
  read_failed: "readFailed",
  copy_failed: "copyFailed",
};

/** Shows one rejected candidate and its localized reason. */
function RejectionRow({
  rejection,
}: {
  rejection: ScheduledRunArtifactRejectionInfo;
}) {
  const { t } = useTranslation();
  const key = REJECTION_TRANSLATION_KEYS[rejection.reason] ?? rejection.reason;
  return (
    <li className="flex items-center justify-between gap-3 text-xs">
      <span className="min-w-0 truncate">{rejection.name}</span>
      <span className="shrink-0 text-muted-foreground">
        {t(`scheduledTask.artifactRejection.${key}`)}
      </span>
    </li>
  );
}
