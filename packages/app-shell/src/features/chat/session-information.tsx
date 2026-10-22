import { useRef, useState } from "react";
import { flushSync } from "react-dom";
import { groupSubagentItems } from "./subagent-items";
import { locateSessionChild } from "./session-child-navigation";
import { useTranslation } from "react-i18next";
import { useStore } from "zustand";
import {
  Button,
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@ora/ui";
import { useChatStore } from "../../chat-store-context";
import { useSessionBindings } from "../../state/data/session-bindings";
import type { SessionBindings } from "@ora/contracts";
import { RunDiagnosticsPanel } from "./run-diagnostics";

/** One panel shared by ordinary conversations and every workflow execution reference. */
export function SessionInformation({ sessionId }: { sessionId?: string }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  return (
    <>
      <Button variant="ghost" size="sm" onClick={() => setOpen(true)}>
        {t("chat.sessionInformation")}
      </Button>
      {open && (
        <SessionInformationDialog
          key={sessionId ?? "new"}
          sessionId={sessionId}
          onClose={() => setOpen(false)}
        />
      )}
    </>
  );
}

/** Unmounting a dialog tears down its scoped query and prevents late adoption in another session. */
export function SessionInformationDialog({
  sessionId,
  onClose,
}: {
  sessionId?: string;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const navigating = useRef(false);
  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
    >
      <DialogContent
        finalFocus={() => (navigating.current ? false : undefined)}
        className="max-h-[85vh] overflow-y-auto sm:max-w-2xl"
      >
        <DialogHeader>
          <DialogTitle>{t("chat.sessionInformation")}</DialogTitle>
        </DialogHeader>
        {sessionId === undefined ? (
          <p>{t("chat.bindingNotCreated")}</p>
        ) : (
          <SessionInformationBody
            key={sessionId}
            sessionId={sessionId}
            onClose={() => {
              navigating.current = true;
              onClose();
            }}
          />
        )}
      </DialogContent>
    </Dialog>
  );
}

/** Reads only when explicitly opened; live observations remain owned by the chat store. */
function SessionInformationBody({
  sessionId,
  onClose,
}: {
  sessionId: string;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const query = useSessionBindings(sessionId);
  const chatStore = useChatStore();
  const conversation = useStore(
    chatStore,
    (state) => state.conversations[sessionId],
  );
  const [locationMissing, setLocationMissing] = useState(false);
  return (
    <div className="space-y-4 text-xs">
      <div>
        <p className="font-medium">Ora Session ID</p>
        <CopyIdentifier value={sessionId} />
      </div>
      <section>
        <h3 className="mb-2 font-medium">{t("chat.bindingTimeline")}</h3>
        {query.isPending ? (
          <p>{t("chat.bindingLoading")}</p>
        ) : query.isError ? (
          <p role="alert">{t("chat.bindingUnavailable")}</p>
        ) : (
          <BindingTimeline result={query.data} />
        )}
        <Button variant="ghost" size="sm" onClick={() => void query.refetch()}>
          {t("chat.bindingRefresh")}
        </Button>
      </section>
      <RunDiagnosticsPanel
        diagnostics={conversation?.diagnostics}
        turns={conversation?.turns ?? []}
        isResponding={conversation?.isResponding ?? false}
        pendingPermissions={conversation?.pendingPermissions ?? []}
      />
      <p className="text-muted-foreground">{t("chat.bindingChildLocation")}</p>
      {conversation?.turns.flatMap((turn) =>
        groupSubagentItems(turn.items).flatMap((item) =>
          item.kind === "subagentGroup"
            ? [
                <Button
                  key={`${turn.id}:${item.id}`}
                  variant="outline"
                  size="sm"
                  onClick={() => {
                    if (
                      !locateSessionChild(
                        sessionId,
                        `${turn.id}:display:${item.id}`,
                      )
                    ) {
                      setLocationMissing(true);
                      return;
                    }
                    flushSync(onClose);
                  }}
                >
                  {t("chat.bindingLocateChildren")} ·{" "}
                  {item.title ?? item.sourceId}
                </Button>,
              ]
            : [],
        ),
      )}
      {locationMissing && (
        <p role="status">{t("chat.bindingChildrenUnavailable")}</p>
      )}
    </div>
  );
}

/** Renders verified segments only; unavailable histories never masquerade as never-created. */
export function BindingTimeline({ result }: { result?: SessionBindings }) {
  const { t } = useTranslation();
  if (result?.type !== "available")
    return (
      <p>
        {t(
          result?.type === "retry"
            ? "chat.bindingRetry"
            : result?.type === "not_found"
              ? "chat.bindingNotFound"
              : "chat.bindingUnavailable",
        )}
      </p>
    );
  return (
    <>
      {result.history !== "readable" && (
        <p role="status">{t(`chat.bindingHistory_${result.history}`)}</p>
      )}
      {result.segments.length === 0 && result.history === "readable" && (
        <p>{t("chat.bindingNotCreated")}</p>
      )}
      <ol className="space-y-3">
        {result.segments.map((segment) => (
          <li key={segment.key} className="rounded border p-2">
            <p className="break-all font-medium">
              {segment.agentRef}{" "}
              {segment.current && <span>· {t("chat.bindingCurrent")}</span>}
            </p>
            <CopyIdentifier value={segment.agentSessionId} />
            <p className="text-muted-foreground">
              {segment.recordedAt === null ? (
                t(
                  segment.key === "live"
                    ? "chat.bindingUnrecorded"
                    : "chat.bindingTimeUnknown",
                )
              ) : (
                <>
                  {t("chat.bindingRecordedAt")}{" "}
                  <time dateTime={segment.recordedAt}>
                    {new Date(segment.recordedAt).toLocaleString()}
                  </time>
                </>
              )}
            </p>
          </li>
        ))}
      </ol>
    </>
  );
}

/** Copies the entire opaque identifier with visible success/failure feedback. */
function CopyIdentifier({ value }: { value: string }) {
  const { t } = useTranslation();
  const [status, setStatus] = useState<"copied" | "copyFailed" | undefined>();
  return (
    <div className="flex items-start gap-2">
      <code className="min-w-0 flex-1 break-all select-text">{value}</code>
      <Button
        variant="outline"
        size="sm"
        onClick={async () => {
          try {
            await navigator.clipboard.writeText(value);
            setStatus("copied");
          } catch {
            setStatus("copyFailed");
          }
        }}
      >
        {t("chat.bindingCopy")}
      </Button>
      {status && <span role="status">{t(`chat.binding_${status}`)}</span>}
    </div>
  );
}
