import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { createChatStore } from "@ora/chat";
import type { InstalledPlugin } from "@ora/contracts";
import { beforeEach, describe, expect, it } from "vitest";
import { PlatformProvider } from "../../platform";
import {
  createHookWrapper,
  createTestQueryClient,
} from "../../test/hook-harness";
import {
  createMockClient,
  createMockClientState,
} from "../../test/mock-client";
import { createSurfaceTestPlatform } from "../../test/surface-test-platform";
import { useSurfaceStore } from "../../state/stores/surface-store";
import { TRACE_DASHBOARD_PLUGIN_ID } from "./constants";
import { useOpenTraceDashboard } from "./use-open-trace-dashboard";

function traceDashboardPlugin(): InstalledPlugin {
  return {
    id: TRACE_DASHBOARD_PLUGIN_ID,
    namespace: "official",
    name: "ora-space.agent-trace-visualizer",
    displayName: "Agent Trace Visualizer",
    description: "Agent trace visualization dashboard",
    homepage: null,
    license: null,
    version: "0.1.0",
    kind: "workbench",
    title: "Agent Trace Visualizer",
    logo: null,
    installationValidity: { validity: "valid" },
    configuration: { state: "not_declared" },
    runtime: "stopped",
  };
}

/** Renders the hook through a probe button and reports whether it resolved. */
function Probe({ sessionId }: { sessionId: string }) {
  const open = useOpenTraceDashboard();
  return (
    <button onClick={() => open?.(sessionId)}>
      {open === null ? "fallback" : "plugin"}
    </button>
  );
}

function renderProbe(plugins: InstalledPlugin[]) {
  const state = createMockClientState();
  state.installedPlugins = plugins;
  const client = createMockClient(state);
  const Wrapper = createHookWrapper(
    client,
    createTestQueryClient(),
    createChatStore(client.session),
  );
  const host = createSurfaceTestPlatform({ embedded: true });
  return {
    host,
    ...render(
      <Wrapper>
        <PlatformProvider adapter={host.platform}>
          <Probe sessionId="sess-7" />
        </PlatformProvider>
      </Wrapper>,
    ),
  };
}

describe("useOpenTraceDashboard", () => {
  beforeEach(() => {
    useSurfaceStore.setState({
      embeddedSupported: true,
      records: {},
      failures: {},
      sidePanelInstance: null,
    });
  });

  it("falls back to the legacy flow when the plugin is not installed", async () => {
    const user = userEvent.setup();
    const { host } = renderProbe([]);

    const button = await screen.findByRole("button", { name: "fallback" });
    await user.click(button);

    expect(host.surfaces.open).not.toHaveBeenCalled();
  });

  it("opens the plugin surface bound to the session when installed", async () => {
    const user = userEvent.setup();
    const { host } = renderProbe([traceDashboardPlugin()]);

    const button = await screen.findByRole("button", { name: "plugin" });
    await user.click(button);

    expect(host.surfaces.open).toHaveBeenCalledWith(
      { pluginId: TRACE_DASHBOARD_PLUGIN_ID },
      "embedded",
      "sess-7",
    );
  });
});
