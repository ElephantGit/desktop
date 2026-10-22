import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "@ora/ui";
import type { ScheduledRunArtifactInfo } from "@ora/contracts";
import { useOptionalPlatform } from "../../platform";

/** Drives one Scheduled Run artifact export at a time. */
export interface ScheduledArtifactDownloadController {
  /** True while the native save flow or copy is still running. */
  readonly isDownloading: boolean;
  /** Opens the native save dialog and copies the artifact; a dismissed dialog is silent. */
  download: (
    runId: string,
    artifact: ScheduledRunArtifactInfo,
  ) => Promise<void>;
}

/** Returns `undefined` when the host cannot export files, so callers hide the affordance. */
export function useScheduledArtifactDownload():
  ScheduledArtifactDownloadController | undefined {
  const { t } = useTranslation();
  const platform = useOptionalPlatform();
  const [isDownloading, setIsDownloading] = useState(false);

  const download = useCallback(
    async (runId: string, artifact: ScheduledRunArtifactInfo) => {
      if (platform?.scheduledArtifacts === undefined) return;
      const segments = artifact.relativePath.split("/");
      const fileName = segments[segments.length - 1] || artifact.relativePath;
      try {
        setIsDownloading(true);
        const destination = await platform.selectSavePath({
          defaultFileName: fileName,
        });
        if (destination === null) return;
        await platform.scheduledArtifacts.download(
          runId,
          artifact.relativePath,
          destination,
        );
        toast.success(t("scheduledTask.artifactDownloaded"));
      } catch {
        toast.error(t("scheduledTask.artifactDownloadFailed"));
      } finally {
        setIsDownloading(false);
      }
    },
    [platform, t],
  );

  return platform?.scheduledArtifacts === undefined
    ? undefined
    : { isDownloading, download };
}
