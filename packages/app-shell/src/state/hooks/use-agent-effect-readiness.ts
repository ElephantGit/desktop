import { useQuery } from "@tanstack/react-query";
import { useContractsClient } from "../../contracts-client-context";
import { effectKeys } from "../data/effects";

export type AgentEffectReadiness = "ready" | "blocked";

// Effect Consumers are agent packages, so these use their canonical package identities rather
// than the legacy bare agent names the picker used before namespaced AgentRefs were introduced.
const EFFECT_MANAGED_AGENT_REFS = new Set([
  "official/ora-space.opencode",
  "official/ora-space.claude",
  "official/ora-space.codex",
]);

/** Gates chat on the complete persisted Effect Target, never on one Resource in isolation. */
export function useAgentEffectReadiness(
  workspaceId: string | undefined,
  agentRef: string | null,
): AgentEffectReadiness {
  const client = useContractsClient();
  const managedAgent =
    agentRef !== null && EFFECT_MANAGED_AGENT_REFS.has(agentRef);
  const query = useQuery({
    queryKey: effectKeys.agentEffectStatus(workspaceId ?? "", agentRef ?? ""),
    queryFn: () =>
      client.effect.getTargetStatus({
        selector: "workspace_agent",
        workspaceId: workspaceId ?? "",
        agentPluginId: agentRef ?? "",
      }),
    enabled: managedAgent && workspaceId !== undefined,
    refetchInterval: 1_000,
  });
  if (!managedAgent || workspaceId === undefined) return "ready";
  const status = query.data?.status;
  // A first prompt must not race the initial status query: no Target evidence is not readiness.
  if (status === undefined) return "blocked";
  if (status === null) return "blocked";
  const current =
    status.phase === "current" || status.phase === "current_with_issues";
  const blocking = status.conditions.some(
    (condition) => condition.impact === "blocking",
  );
  return current &&
    status.readyGeneration >= status.desiredGeneration &&
    !blocking
    ? "ready"
    : "blocked";
}
