import type { InstalledPlugin } from "@ora/contracts";
import type { WorkflowMcpChoice, WorkflowNodeData } from "@ora/workflow-mock";
import type { Node } from "@xyflow/react";

/** Identifies one authoring binding that cannot become a canonical runtime Plugin ID. */
export interface InvalidWorkflowMcpBinding {
  nodeId: string;
  nodeTitle: string;
  mcpId: string;
}

/** Applies the domain's structural `<namespace>/<name>` grammar without consulting installation. */
export function isCanonicalPluginId(value: string): boolean {
  const segments = value.split("/");
  if (segments.length !== 2) return false;
  const [namespace, name] = segments;
  if (
    namespace.length === 0 ||
    namespace.length > 33 ||
    name.length === 0 ||
    namespace === "." ||
    namespace === ".." ||
    name === "." ||
    name === ".."
  ) {
    return false;
  }
  return /^[a-z0-9.-]+$/.test(namespace) && /^[a-z0-9.-]+$/.test(name);
}

/** Lists malformed persisted bindings so publication can explain every required repair at once. */
export function invalidWorkflowMcpBindings(
  nodes: readonly Node<WorkflowNodeData, "workflow">[],
): InvalidWorkflowMcpBinding[] {
  return nodes.flatMap((node) =>
    (node.data.agentConfig?.mcps ?? [])
      .filter((binding) => !isCanonicalPluginId(binding.mcpId))
      .map((binding) => ({
        nodeId: node.id,
        nodeTitle: node.data.title,
        mcpId: binding.mcpId,
      })),
  );
}

/** Maps installed identities and secret-free readiness to the authoring catalog. */
export function workflowMcpChoices(
  plugins: readonly InstalledPlugin[],
): WorkflowMcpChoice[] {
  return plugins
    .filter((plugin) => plugin.kind === "mcp")
    .map((plugin) => {
      const choice = { value: plugin.id, label: plugin.displayName };
      if (plugin.installationValidity.validity !== "valid") {
        return { ...choice, unavailableReason: "invalidDeclaration" };
      }
      if (plugin.configuration.state === "unavailable") {
        return { ...choice, unavailableReason: "configurationUnavailable" };
      }
      if (
        plugin.configuration.state === "available" &&
        plugin.configuration.completeness !== "complete"
      ) {
        return { ...choice, unavailableReason: "configurationIncomplete" };
      }
      return choice;
    });
}

/** Loading failures must not turn persisted bindings into allegedly uninstalled plugins. */
export interface WorkflowMcpCatalogStatus {
  isLoading: boolean;
  isError: boolean;
  onRetry: () => void;
}
