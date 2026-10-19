import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useContractsClient } from "../../contracts-client-context";
import { recoverEffectTarget } from "../data/effects";

/** Exposes the effects owner's recovery operation and cache adoption to callers. */
export function useRecoverEffectTarget() {
  const client = useContractsClient();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (targetId: string) =>
      recoverEffectTarget(client, queryClient, targetId),
  });
}
