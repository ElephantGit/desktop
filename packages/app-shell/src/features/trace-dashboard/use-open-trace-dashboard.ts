import { useMemo } from "react";
import { useInstalledPlugins } from "../../state/hooks/use-installed-plugins";
import { useOpenSurface } from "../surface/use-open-surface";
import { listSurfaceDefinitions } from "../surface/surface-definitions";
import { TRACE_DASHBOARD_PLUGIN_ID } from "./constants";

/**
 * Opens the trace dashboard's workbench surface bound to the given Ora session.
 *
 * Returns `null` while the installed-plugins snapshot is still loading or when
 * the plugin is not installed, so callers can fall back to the legacy dashboard
 * flow. Routing here — in the click handler — means the legacy sheet never mounts
 * on the plugin path and no close/open race can leave its overlay behind.
 */
export function useOpenTraceDashboard():
  ((sessionId?: string | null) => void) | null {
  const installedPlugins = useInstalledPlugins().data;
  const openSurface = useOpenSurface();
  return useMemo(() => {
    if (installedPlugins === undefined) return null;
    const available = listSurfaceDefinitions(installedPlugins).some(
      (definition) => definition.pluginId === TRACE_DASHBOARD_PLUGIN_ID,
    );
    if (!available) return null;
    return (sessionId?: string | null) => {
      void openSurface(
        { pluginId: TRACE_DASHBOARD_PLUGIN_ID },
        sessionId ?? undefined,
      );
    };
  }, [installedPlugins, openSurface]);
}
