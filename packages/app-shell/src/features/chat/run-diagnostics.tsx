import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { ChatTurn, RunDiagnostics } from "@ora/chat";
import type { SessionPermissionRequest } from "@ora/contracts";

/** Shows observations separately from parent progress; silence is never rendered as health. */
export function RunDiagnosticsPanel({
  diagnostics,
  turns,
  isResponding,
  pendingPermissions,
}: {
  diagnostics?: RunDiagnostics;
  turns: ChatTurn[];
  isResponding: boolean;
  pendingPermissions: SessionPermissionRequest[];
}) {
  const { t } = useTranslation();
  const [now, setNow] = useState(Date.now);
  const lastActivity = diagnostics?.lastActivity;
  useEffect(() => {
    if (!isResponding || lastActivity === undefined) return;
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [isResponding, lastActivity]);
  const tools = isResponding
    ? (turns
        .at(-1)
        ?.items.filter(
          (item) =>
            item.kind === "toolCall" &&
            (item.status === "pending" || item.status === "in_progress"),
        ) ?? [])
    : [];
  const provider = diagnostics?.provider;
  const report =
    provider?.kind === "retry"
      ? t("chat.diagnosticsRetry", { attempt: provider.attempt })
      : provider?.kind === "compaction"
        ? t(`chat.diagnosticsCompaction_${provider.status}`)
        : t("chat.diagnosticsUnknown");
  return (
    <details
      className="mb-2 rounded-md border px-3 py-2 text-xs"
      data-testid="run-diagnostics"
    >
      <summary className="cursor-pointer text-muted-foreground">
        {t("chat.diagnosticsTitle")}
      </summary>
      <dl className="mt-2 grid grid-cols-[auto_1fr] gap-x-3 gap-y-1.5">
        <dt>{t("chat.diagnosticsParent")}</dt>
        <dd>
          {isResponding
            ? t("chat.diagnosticsActive")
            : t("chat.diagnosticsInactive")}
        </dd>
        <dt>{t("chat.diagnosticsProvider")}</dt>
        <dd>{report}</dd>
        <dt>{t("chat.diagnosticsLastActivity")}</dt>
        <dd>
          {lastActivity === undefined ? (
            t("chat.diagnosticsNoLiveTime")
          ) : (
            <>
              {t(`chat.diagnosticsActivity_${lastActivity.kind}`)} ·{" "}
              <time dateTime={new Date(lastActivity.receivedAt).toISOString()}>
                {new Date(lastActivity.receivedAt).toLocaleString()}
              </time>
              {isResponding && (
                <>
                  {" "}
                  ·{" "}
                  {t("chat.diagnosticsSilence", {
                    seconds: Math.max(
                      0,
                      Math.floor((now - lastActivity.receivedAt) / 1000),
                    ),
                  })}
                </>
              )}
            </>
          )}
        </dd>
        <dt>{t("chat.diagnosticsRequests")}</dt>
        <dd>
          {pendingPermissions.length
            ? t("chat.diagnosticsWaiting", { count: pendingPermissions.length })
            : t("chat.diagnosticsNoRequests")}
        </dd>
        <dt>{t("chat.diagnosticsTools")}</dt>
        <dd>
          {tools.length
            ? tools
                .map((tool) => (tool.kind === "toolCall" ? tool.title : ""))
                .join(" · ")
            : t("chat.diagnosticsNoTools")}
        </dd>
      </dl>
      <p className="mt-2 text-muted-foreground">{t("chat.diagnosticsLimit")}</p>
    </details>
  );
}
