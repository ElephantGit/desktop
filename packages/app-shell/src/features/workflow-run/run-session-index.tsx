import { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Button,
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@ora/ui";
import type { WorkflowSessionExecution } from "../../state/data/workflow-session-executions";
import {
  BindingTimeline,
  SessionInformationDialog,
} from "../chat/session-information";
import { useSessionBindingBatch } from "../../state/data/session-bindings";

/** Run-only, opt-in execution index. A shared session is fetched once but never merges rows. */
export function RunSessionIndex({
  runId,
  executions,
}: {
  runId: string;
  executions: WorkflowSessionExecution[];
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  return (
    <>
      <Button variant="ghost" size="sm" onClick={() => setOpen(true)}>
        {t("chat.bindingRunIndex")}
      </Button>
      {open && (
        <IndexDialog
          key={runId}
          runId={runId}
          executions={executions}
          onClose={() => setOpen(false)}
        />
      )}
    </>
  );
}

/** Scoped batch queries and selection disappear when the run changes or the index closes. */
function IndexDialog({
  runId,
  executions,
  onClose,
}: {
  runId: string;
  executions: WorkflowSessionExecution[];
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const [selected, setSelected] = useState<string>();
  const query = useSessionBindingBatch(
    runId,
    executions.flatMap((row) => (row.sessionId ? [row.sessionId] : [])),
  );
  const bindings = new Map(
    (query.data ?? []).map((item) => [item.sessionId, item]),
  );
  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
    >
      <DialogContent className="max-h-[85vh] overflow-auto sm:max-w-5xl">
        <DialogHeader>
          <DialogTitle>{t("chat.bindingRunIndex")}</DialogTitle>
        </DialogHeader>
        {query.isPending && <p>{t("chat.bindingLoading")}</p>}
        {query.isError && <p role="alert">{t("chat.bindingUnavailable")}</p>}
        <Button variant="ghost" size="sm" onClick={() => void query.refetch()}>
          {t("chat.bindingRefresh")}
        </Button>
        <table className="w-full text-left text-xs">
          <thead>
            <tr>
              {[
                "bindingNode",
                "bindingRound",
                "bindingAttempt",
                "bindingStatus",
                "bindingSessions",
              ].map((key) => (
                <th className="p-2" key={key}>
                  {t(`chat.${key}`)}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {executions.map((row) => (
              <tr key={row.key} className="align-top border-t">
                <td className="p-2">
                  <p>{row.nodeId}</p>
                  <small className="break-all text-muted-foreground">
                    {row.scopeId}
                  </small>
                </td>
                <td className="p-2">
                  {row.iteration === undefined &&
                  row.loopRound === undefined ? (
                    "—"
                  ) : (
                    <>
                      {row.iteration !== undefined && (
                        <p>
                          {t("workflowRun.theater.roundOption", {
                            round: row.iteration + 1,
                          })}
                        </p>
                      )}
                      {row.loopRound !== undefined && (
                        <p>
                          {t("workflowRun.loopRounds.round", {
                            round: row.loopRound,
                          })}
                        </p>
                      )}
                    </>
                  )}
                </td>
                <td className="p-2">
                  {row.attempt ?? t("chat.bindingUnknown")}
                </td>
                <td className="p-2">
                  {t(`workflowRun.status.${row.status}`, {
                    defaultValue: row.status,
                  })}
                </td>
                <td className="min-w-64 p-2">
                  {row.sessionId ? (
                    <>
                      <Button
                        variant="link"
                        size="sm"
                        className="max-w-full break-all whitespace-normal"
                        onClick={() => setSelected(row.sessionId)}
                      >
                        {row.sessionId}
                      </Button>
                      {query.isPending ? (
                        <p>{t("chat.bindingLoading")}</p>
                      ) : (
                        <details>
                          <summary className="cursor-pointer">
                            {t("chat.bindingTimeline")}
                          </summary>
                          <BindingTimeline
                            result={bindings.get(row.sessionId)}
                          />
                        </details>
                      )}
                    </>
                  ) : (
                    t("chat.bindingNoSession")
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        {selected && (
          <SessionInformationDialog
            key={selected}
            sessionId={selected}
            onClose={() => setSelected(undefined)}
          />
        )}
      </DialogContent>
    </Dialog>
  );
}
