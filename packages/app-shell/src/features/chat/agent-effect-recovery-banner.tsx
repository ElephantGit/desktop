import { useTranslation } from "react-i18next";
import { IconRefresh } from "@tabler/icons-react";
import { useRecoverEffectTarget } from "../../state/hooks/use-recover-effect-target";
import { localizeContractError } from "../../i18n/contract-error";
import type { AgentEffectReadiness } from "../../state/hooks/use-agent-effect-readiness";

/**
 * Offers the controlled recovery exit while chat is gated by an Effect quarantine.
 *
 * Rendering nothing unless the failure is the recovery quarantine keeps callers free
 * to mount this unconditionally beside the conversation it belongs to: other manual
 * Conditions need different remedies and must not offer a recovery that cannot help.
 */
export function AgentEffectRecoveryBanner({
  readiness,
}: {
  readiness: AgentEffectReadiness;
}) {
  const { t } = useTranslation();
  const recovery = useRecoverEffectTarget();

  if (
    readiness.kind !== "failed" ||
    !readiness.recoveryRequired ||
    readiness.targetId === null
  ) {
    return null;
  }
  // The condition message is backend-approved safe detail (SafeConditionDetails), not a
  // raw Error.message, so rendering it verbatim is the same contract the send hint uses.
  const conditionMessage = readiness.message;
  const targetId = readiness.targetId;
  return (
    <div
      role="alert"
      // Names the state without depending on the localized sentence, so tests and
      // bug reports can tell a refused recovery apart from a pending one.
      data-effect-recovery={
        recovery.isPending
          ? "pending"
          : recovery.isError
            ? "failed"
            : "required"
      }
      className="mx-3 mb-2 flex items-start gap-2 rounded-md border border-red-500/30 bg-red-500/5 px-3 py-2 text-xs sm:mx-4"
    >
      <IconRefresh
        className="mt-0.5 size-4 shrink-0 text-red-600 dark:text-red-400"
        aria-hidden="true"
      />
      <div className="min-w-0 flex-1">
        <p className="font-medium text-red-700 dark:text-red-300">
          {t("chat.agentEffectRecovery.title")}
        </p>
        {/* The backend's condition message is the only description of what actually
            broke, so it is shown verbatim rather than flattened into one sentence. */}
        <p className="mt-0.5 break-words text-muted-foreground">
          {conditionMessage}
        </p>
        {recovery.isError && (
          <p className="mt-0.5 break-words text-red-700 dark:text-red-300">
            {t("chat.agentEffectRecovery.refused", {
              message: localizeContractError(recovery.error, t),
            })}
          </p>
        )}
      </div>
      <button
        type="button"
        disabled={recovery.isPending}
        onClick={() => recovery.mutate(targetId)}
        className="shrink-0 rounded-md border border-red-500/40 px-2.5 py-1 font-medium text-red-700 transition-colors hover:bg-red-500/10 disabled:opacity-50 dark:text-red-300"
      >
        {recovery.isPending
          ? t("chat.agentEffectRecovery.pending")
          : t("chat.agentEffectRecovery.action")}
      </button>
    </div>
  );
}
