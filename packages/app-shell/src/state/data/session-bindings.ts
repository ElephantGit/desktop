import { useQuery } from "@tanstack/react-query";
import { useContractsClient } from "../../contracts-client-context";
import { sessionKeys } from "./sessions";

/** On-demand projection: closing its last consumer aborts IO and discards the cached ids. */
export function useSessionBindings(sessionId: string) {
  const client = useContractsClient();
  return useQuery({
    queryKey: sessionKeys.bindings(sessionId),
    queryFn: ({ signal }) =>
      client.session
        .bindings({ sessionIds: [sessionId] }, { signal })
        .then((response) =>
          response.sessions.find((item) => item.sessionId === sessionId),
        ),
    gcTime: 0,
    staleTime: 0,
    retry: false,
  });
}

/** Serializes capped batches; one failed batch leaves other executions available. */
export function useSessionBindingBatch(runId: string, sessionIds: string[]) {
  const client = useContractsClient();
  const ids = [...new Set(sessionIds)].sort();
  return useQuery({
    queryKey: sessionKeys.bindingBatch(runId, ids),
    queryFn: async ({ signal }) => {
      const result: import("@ora/contracts").SessionBindings[] = [];
      for (let index = 0; index < ids.length; index += 32) {
        signal.throwIfAborted();
        const batch = ids.slice(index, index + 32);
        try {
          result.push(
            ...(
              await client.session.bindings({ sessionIds: batch }, { signal })
            ).sessions,
          );
        } catch {
          signal.throwIfAborted();
          result.push(
            ...batch.map((sessionId) => ({
              type: "unavailable" as const,
              sessionId,
            })),
          );
        }
      }
      return result;
    },
    gcTime: 0,
    staleTime: 0,
    retry: false,
  });
}
