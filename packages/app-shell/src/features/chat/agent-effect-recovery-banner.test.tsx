import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { createElement } from "react";
import type { ContractsClient } from "@ora/contracts";
import { QueryClient } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { createTestClient } from "../../test/contracts-transport";
import {
  createHookWrapper,
  createTestQueryClient,
} from "../../test/hook-harness";
import { createChatStore } from "@ora/chat";
import "../../i18n/i18n-instance";
import type { RecoverEffectTargetResponse } from "@ora/contracts";
import type { AgentEffectReadiness } from "../../state/hooks/use-agent-effect-readiness";
import { AgentEffectRecoveryBanner } from "./agent-effect-recovery-banner";
import { useAgentEffectReadiness } from "../../state/hooks/use-agent-effect-readiness";
import { AGENT_REF } from "../../test/agent-identity";
import { readyEffectHandlers } from "../../test/memory/effects";
import { createPluginMemory, pluginHandlers } from "../../test/memory/plugins";
import type { GetEffectTargetStatusRequest } from "@ora/contracts";

/** The failed readiness a quarantined Effect Target produces. */
const quarantined: AgentEffectReadiness = {
  kind: "failed",
  message: "An Effect operation requires explicit recovery.",
  targetId: "target-1",
  recoveryRequired: true,
};

/** Mounts the banner with its providers but without any workspace-specific state. */
function renderBanner(
  readiness: AgentEffectReadiness,
  client: ContractsClient,
  queryClient?: QueryClient,
) {
  const queries = queryClient ?? createTestQueryClient();
  const wrapper = createHookWrapper(
    client,
    queries,
    createChatStore(client.session),
  );
  return {
    ...render(createElement(AgentEffectRecoveryBanner, { readiness }), {
      wrapper,
    }),
    queries,
  };
}

describe("AgentEffectRecoveryBanner", () => {
  it("refreshes real readiness after recovery and isolates other workspaces and agents", async () => {
    const ready = await readyEffectHandlers().getEffectTargetStatus();
    let recovered = false;
    const getEffectTargetStatus = vi.fn(
      async (request: GetEffectTargetStatusRequest) => {
        if (request.selector !== "workspace_agent")
          throw new Error("expected workspace selector");
        const selected =
          request.workspaceId === "workspace-1" &&
          request.agentPluginId === AGENT_REF.claude;
        return {
          status: {
            ...ready.status,
            targetId: selected
              ? "target-1"
              : `${request.workspaceId}/${request.agentPluginId}`,
            phase:
              selected && !recovered
                ? ("recovery_required" as const)
                : ("current" as const),
          },
        };
      },
    );
    const recoverEffectTarget = vi.fn(async (request: { targetId: string }) => {
      expect(request).toEqual({ targetId: "target-1" });
      recovered = true;
      return {
        recovered: true,
        finalizedOperations: 1,
        closedWithoutEffectOperations: 0,
      };
    });
    const client = createTestClient({
      ...pluginHandlers(createPluginMemory()),
      getEffectTargetStatus,
      recoverEffectTarget,
    });
    function Gate({
      workspaceId,
      agentRef,
    }: {
      workspaceId: string;
      agentRef: string;
    }) {
      const readiness = useAgentEffectReadiness(workspaceId, agentRef);
      return (
        <>
          <button
            aria-label={`send ${workspaceId} ${agentRef}`}
            disabled={readiness.kind !== "ready"}
          >
            Send
          </button>
          <AgentEffectRecoveryBanner readiness={readiness} />
        </>
      );
    }
    render(
      <>
        <Gate workspaceId="workspace-1" agentRef={AGENT_REF.claude} />
        <Gate workspaceId="workspace-2" agentRef={AGENT_REF.claude} />
        <Gate workspaceId="workspace-1" agentRef={AGENT_REF.opencode} />
      </>,
      {
        wrapper: createHookWrapper(
          client,
          createTestQueryClient(),
          createChatStore(client.session),
        ),
      },
    );
    const send = screen.getByRole("button", {
      name: `send workspace-1 ${AGENT_REF.claude}`,
    });
    await screen.findByRole("alert");
    await waitFor(() => expect(getEffectTargetStatus).toHaveBeenCalledTimes(3));
    expect(send).toBeDisabled();
    await userEvent.click(screen.getByRole("button", { name: /恢复|Recover/ }));
    await waitFor(() => expect(send).toBeEnabled());
    expect(recoverEffectTarget).toHaveBeenCalledOnce();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(
      getEffectTargetStatus.mock.calls.map(([request]) => request),
    ).toEqual([
      {
        selector: "workspace_agent",
        workspaceId: "workspace-1",
        agentPluginId: AGENT_REF.claude,
      },
      {
        selector: "workspace_agent",
        workspaceId: "workspace-2",
        agentPluginId: AGENT_REF.claude,
      },
      {
        selector: "workspace_agent",
        workspaceId: "workspace-1",
        agentPluginId: AGENT_REF.opencode,
      },
      {
        selector: "workspace_agent",
        workspaceId: "workspace-1",
        agentPluginId: AGENT_REF.claude,
      },
    ]);
  });
  it("renders nothing unless the failure is the manual-recovery quarantine", () => {
    const client = createTestClient({});
    const { container } = renderBanner({ kind: "ready" }, client);
    expect(container).toBeEmptyDOMElement();
    renderBanner({ kind: "blocked" }, client);
    renderBanner(
      {
        kind: "failed",
        message: "A Managed Item changed outside Ora.",
        targetId: "target-1",
        recoveryRequired: false,
      },
      client,
    );
    renderBanner(
      {
        kind: "failed",
        message: "An Effect operation requires explicit recovery.",
        targetId: null,
        recoveryRequired: true,
      },
      client,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("keeps the quarantine visible and explains a refused recovery", async () => {
    const recoverEffectTarget = vi.fn(async () => {
      throw new Error(
        "operation operation-1 on Resource resource-1 proves neither exact state",
      );
    });
    const client = createTestClient({ recoverEffectTarget });

    renderBanner(quarantined, client);
    await userEvent.click(screen.getByRole("button", { name: /恢复|Recover/ }));

    const banner = await screen.findByRole("alert");
    expect(banner.dataset.effectRecovery).toBe("failed");
    expect(banner).toHaveTextContent(/恢复未完成|Recovery did not complete/);
  });

  it("keeps the real send gate failed when recovery is refused", async () => {
    const ready = await readyEffectHandlers().getEffectTargetStatus();
    const getEffectTargetStatus = vi.fn(async () => ({
      status: {
        ...ready.status,
        targetId: "target-1",
        phase: "recovery_required" as const,
      },
    }));
    const client = createTestClient({
      ...pluginHandlers(createPluginMemory()),
      getEffectTargetStatus,
      recoverEffectTarget: async () => {
        throw new Error("recovery refused");
      },
    });
    function Gate() {
      const readiness = useAgentEffectReadiness(
        "workspace-1",
        AGENT_REF.claude,
      );
      return (
        <>
          <button aria-label="send" disabled={readiness.kind !== "ready"}>
            Send
          </button>
          <AgentEffectRecoveryBanner readiness={readiness} />
        </>
      );
    }
    render(<Gate />, {
      wrapper: createHookWrapper(
        client,
        createTestQueryClient(),
        createChatStore(client.session),
      ),
    });
    await screen.findByRole("alert");
    await userEvent.click(screen.getByRole("button", { name: /恢复|Recover/ }));
    await waitFor(() =>
      expect(screen.getByRole("alert").dataset.effectRecovery).toBe("failed"),
    );
    expect(screen.getByRole("button", { name: "send" })).toBeDisabled();
    expect(getEffectTargetStatus).toHaveBeenCalledOnce();
  });

  it("exposes the pending state while recovery is in flight", async () => {
    // A holder keeps the resolver reachable after the closure assigns it; a bare
    // let-binding narrows to its null initializer across the callback boundary.
    const pending: {
      resolve: ((value: RecoverEffectTargetResponse) => void) | null;
    } = { resolve: null };
    const recoverEffectTarget = vi.fn(
      () =>
        new Promise<RecoverEffectTargetResponse>((resolve) => {
          pending.resolve = resolve;
        }),
    );
    const client = createTestClient({ recoverEffectTarget });

    renderBanner(quarantined, client);
    const banner = screen.getByRole("alert");
    expect(banner.dataset.effectRecovery).toBe("required");
    const click = userEvent.click(
      screen.getByRole("button", { name: /恢复|Recover/ }),
    );
    await waitFor(() => expect(banner.dataset.effectRecovery).toBe("pending"));
    pending.resolve?.({
      recovered: true,
      finalizedOperations: 1,
      closedWithoutEffectOperations: 0,
    });
    await click;
    await waitFor(() => expect(banner.dataset.effectRecovery).toBe("required"));
  });
});
