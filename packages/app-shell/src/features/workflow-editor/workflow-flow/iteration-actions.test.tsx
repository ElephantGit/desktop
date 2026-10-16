import { memo } from "react";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Edge, Node } from "@xyflow/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  createMockWorkflowCapabilities,
  type WorkflowNodeData,
} from "@ora/workflow-mock";
import { AppI18nProvider } from "../../../i18n/i18n";
import { appI18n } from "../../../i18n/i18n-instance";
import {
  IterationInsertMenu,
  WorkflowIterationActionsProvider,
} from "./iteration-actions";
import { useWorkflowIterationActions } from "./iteration-actions-context";

function EdgeInsertionProbe({ edge }: { edge: Edge }) {
  const insertion = useWorkflowIterationActions().insertionForEdge(edge);
  return (
    <output data-testid="edge-insertion">{JSON.stringify(insertion)}</output>
  );
}

const StructureProbe = memo(function StructureProbe({
  id,
  renders,
}: {
  id: string;
  renders: { current: Record<string, number> };
}) {
  const count = renders.current;
  // Render counter: the assertion is that memo skips this component.
  // eslint-disable-next-line react-hooks/immutability -- test probe, not component state
  count[id] = (count[id] ?? 0) + 1;
  useWorkflowIterationActions();
  return null;
});

describe("iteration insert actions", () => {
  beforeEach(async () => {
    await appI18n.changeLanguage("en-US");
  });

  it("offers only capability-declared iteration node types", async () => {
    const user = userEvent.setup();
    const onInsert = vi.fn();
    render(
      <AppI18nProvider>
        <WorkflowIterationActionsProvider
          capabilities={createMockWorkflowCapabilities("en-US")}
          nodes={[]}
          edges={[]}
          readOnly={false}
          onInsert={onInsert}
          onToggleCollapsed={vi.fn()}
        >
          <IterationInsertMenu
            insertion={{ type: "entry", iterationId: "iter" }}
            label="Add iteration node"
          />
        </WorkflowIterationActionsProvider>
      </AppI18nProvider>,
    );

    await user.click(
      screen.getByRole("button", { name: "Add iteration node" }),
    );
    expect(
      await screen.findByRole("menuitem", { name: "Agent" }),
    ).toBeVisible();
    expect(screen.getByRole("menuitem", { name: "Condition" })).toBeVisible();
    expect(screen.queryByRole("menuitem", { name: "Start" })).toBeNull();

    await user.click(screen.getByRole("menuitem", { name: "Agent" }));
    expect(onInsert).toHaveBeenCalledWith("agent", {
      type: "entry",
      iterationId: "iter",
    });
  });

  it("does not duplicate the start affordance on an iteration entry edge", () => {
    const nodes: Node<WorkflowNodeData, "workflow">[] = [
      {
        id: "iter",
        type: "workflow",
        position: { x: 0, y: 0 },
        data: { kind: "iteration", title: "Iteration", description: "" },
      },
      {
        id: "body",
        type: "workflow",
        parentId: "iter",
        position: { x: 120, y: 100 },
        data: { kind: "agent", title: "Body", description: "" },
      },
    ];
    const edge: Edge = {
      id: "entry",
      source: "iter",
      sourceHandle: "iteration-entry",
      target: "body",
    };

    render(
      <WorkflowIterationActionsProvider
        capabilities={createMockWorkflowCapabilities("en-US")}
        nodes={nodes}
        edges={[edge]}
        readOnly={false}
        onInsert={vi.fn()}
        onToggleCollapsed={vi.fn()}
      >
        <EdgeInsertionProbe edge={edge} />
      </WorkflowIterationActionsProvider>,
    );

    expect(screen.getByTestId("edge-insertion")).toHaveTextContent("null");
  });

  it("keeps other cards from redrawing when only node config changes", () => {
    const renders = { current: {} as Record<string, number> };
    const capabilities = createMockWorkflowCapabilities("en-US");
    const node = (title: string): Node<WorkflowNodeData, "workflow"> => ({
      id: "agent",
      type: "workflow",
      position: { x: 0, y: 0 },
      data: { kind: "agent", title, description: "" },
    });
    const view = (title: string) => (
      <WorkflowIterationActionsProvider
        capabilities={capabilities}
        nodes={[node(title)]}
        edges={[]}
        readOnly={false}
        onInsert={vi.fn()}
        onToggleCollapsed={vi.fn()}
      >
        <StructureProbe id="card" renders={renders} />
      </WorkflowIterationActionsProvider>
    );
    const { rerender } = render(view("Case"));
    const afterMount = renders.current.card ?? 0;
    rerender(view("Case with another branch"));
    expect(renders.current.card).toBe(afterMount);

    rerender(
      <WorkflowIterationActionsProvider
        capabilities={capabilities}
        nodes={[node("Case with another branch")]}
        edges={[{ id: "e1", source: "agent", target: "out" }]}
        readOnly={false}
        onInsert={vi.fn()}
        onToggleCollapsed={vi.fn()}
      >
        <StructureProbe id="card" renders={renders} />
      </WorkflowIterationActionsProvider>,
    );
    expect(renders.current.card).toBeGreaterThan(afterMount);
  });
});
