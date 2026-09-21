import { act, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { GetEffectTargetStatusRequest } from "@ora/contracts";
import { createTestClient } from "../../test/contracts-transport";
import { renderHookWithClient } from "../../test/hook-harness";
import { AGENT_REF } from "../../test/agent-identity";
import { useAgentEffectReadiness } from "./use-agent-effect-readiness";

/** Verifies that every Effect-managed agent addresses its own canonical Consumer Target. */
async function readinessFor(agentRef: string) {
  const getEffectTargetStatus = vi.fn(
    async (request: GetEffectTargetStatusRequest) => {
      void request;
      return { status: null };
    },
  );
  const { result } = renderHookWithClient(
    () => useAgentEffectReadiness("workspace-1", agentRef),
    createTestClient({ getEffectTargetStatus }),
  );
  await waitFor(() => expect(result.current).toBe("blocked"));
  return getEffectTargetStatus;
}

describe("useAgentEffectReadiness", () => {
  it.each([AGENT_REF.opencode, AGENT_REF.claude, AGENT_REF.codex])(
    "waits for the %s Workspace Target using its canonical plugin identity",
    async (agentRef) => {
      const getEffectTargetStatus = await readinessFor(agentRef);

      expect(getEffectTargetStatus).toHaveBeenCalledOnce();
      expect(getEffectTargetStatus.mock.calls[0]![0]).toEqual({
        selector: "workspace_agent",
        workspaceId: "workspace-1",
        agentPluginId: agentRef,
      });
    },
  );

  it("does not gate agents without an Effect materialization contract", () => {
    const getEffectTargetStatus = vi.fn();
    const { result } = renderHookWithClient(
      () => useAgentEffectReadiness("workspace-1", AGENT_REF.nga),
      createTestClient({ getEffectTargetStatus }),
    );

    expect(result.current).toBe("ready");
    expect(getEffectTargetStatus).not.toHaveBeenCalled();
  });

  it("blocks the first prompt while the Target status request is pending", async () => {
    const getEffectTargetStatus = vi.fn(() => new Promise<never>(() => {}));
    const { result } = renderHookWithClient(
      () => useAgentEffectReadiness("workspace-1", AGENT_REF.claude),
      createTestClient({ getEffectTargetStatus }),
    );

    await act(async () => {});
    expect(result.current).toBe("blocked");
  });
});
