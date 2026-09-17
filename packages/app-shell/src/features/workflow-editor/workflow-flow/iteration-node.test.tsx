import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, render, screen } from "@testing-library/react";
import { useState } from "react";
import {
  applyNodeChanges,
  type Node,
  type NodeChange,
  ReactFlow,
  ReactFlowProvider,
} from "@xyflow/react";
import {
  createMockWorkflowCapabilities,
  WORKFLOW_ITERATION_NODE_HEIGHT,
  WORKFLOW_ITERATION_NODE_WIDTH,
  type WorkflowNodeData,
} from "@ora/workflow-mock";
import { AppI18nProvider } from "../../../i18n/i18n";
import { appI18n } from "../../../i18n/i18n-instance";
import { WorkflowIterationActionsProvider } from "./iteration-actions";
import { WorkflowConnectionStateProvider } from "./connection-state";
import { WorkflowFlowNodeView } from "./node";
import { applyIterationFrameResize } from "../workflow-iteration-graph";

const nodeTypes = { workflow: WorkflowFlowNodeView };

/** jsdom never runs layout, and d3 drag needs a window-scoped event view. */
function windowedMouseEvent(
  type: "mousedown" | "mousemove" | "mouseup",
  init: { button?: number; clientX: number; clientY: number },
): MouseEvent {
  const event = new MouseEvent(type, {
    bubbles: true,
    cancelable: true,
    ...init,
  });
  Object.defineProperty(event, "view", { value: window });
  return event;
}

/** Renders the production custom node inside React Flow so handle registration is exercised. */
function renderIteration({
  collapsed = false,
  readOnly = false,
}: {
  collapsed?: boolean;
  readOnly?: boolean;
} = {}) {
  const node: Node<WorkflowNodeData, "workflow"> = {
    id: "iter",
    type: "workflow",
    position: { x: 40, y: 40 },
    initialWidth: 560,
    initialHeight: 340,
    data: {
      kind: "iteration",
      title: "Iteration",
      description: "Hidden in the inspector",
      collapsed,
      regionMemberCount: 2,
    },
  };
  return render(
    <AppI18nProvider>
      <div style={{ width: 800, height: 600 }}>
        <ReactFlowProvider>
          <WorkflowConnectionStateProvider
            value={{
              connectionCandidateEndpoint: null,
              connectionCandidateNodeId: null,
            }}
          >
            <WorkflowIterationActionsProvider
              capabilities={createMockWorkflowCapabilities("en-US")}
              nodes={[node]}
              edges={[]}
              readOnly={readOnly}
              onInsert={vi.fn()}
              onToggleCollapsed={vi.fn()}
            >
              <ReactFlow
                nodes={[node]}
                edges={[]}
                nodeTypes={nodeTypes}
                nodesConnectable={!readOnly}
              />
            </WorkflowIterationActionsProvider>
          </WorkflowConnectionStateProvider>
        </ReactFlowProvider>
      </div>
    </AppI18nProvider>,
  );
}

/** Renders an editable frame whose node changes are applied like the editor does. */
function renderEditableIteration() {
  function EditableFlow() {
    const [nodes, setNodes] = useState<Node<WorkflowNodeData, "workflow">[]>([
      {
        id: "iter",
        type: "workflow",
        position: { x: 40, y: 40 },
        initialWidth: WORKFLOW_ITERATION_NODE_WIDTH,
        initialHeight: WORKFLOW_ITERATION_NODE_HEIGHT,
        data: {
          kind: "iteration",
          title: "Iteration",
          description: "",
          regionMemberCount: 0,
        },
      },
    ]);
    return (
      <WorkflowConnectionStateProvider
        value={{
          connectionCandidateEndpoint: null,
          connectionCandidateNodeId: null,
        }}
      >
        <WorkflowIterationActionsProvider
          capabilities={createMockWorkflowCapabilities("en-US")}
          nodes={nodes}
          edges={[]}
          readOnly={false}
          onInsert={vi.fn()}
          onToggleCollapsed={vi.fn()}
        >
          <ReactFlow
            nodes={nodes}
            edges={[]}
            nodeTypes={nodeTypes}
            nodesConnectable
            onNodesChange={(
              changes: NodeChange<Node<WorkflowNodeData, "workflow">>[],
            ) => {
              setNodes((current) =>
                applyIterationFrameResize(
                  applyNodeChanges(changes, current),
                  changes,
                ),
              );
            }}
          />
        </WorkflowIterationActionsProvider>
      </WorkflowConnectionStateProvider>
    );
  }
  return render(
    <AppI18nProvider>
      <div style={{ width: 800, height: 600 }}>
        <ReactFlowProvider>
          <EditableFlow />
        </ReactFlowProvider>
      </div>
    </AppI18nProvider>,
  );
}

describe("iteration composite node", () => {
  beforeEach(async () => {
    await appI18n.changeLanguage("en-US");
    Object.defineProperty(HTMLElement.prototype, "clientWidth", {
      configurable: true,
      get: () => 800,
    });
    Object.defineProperty(HTMLElement.prototype, "clientHeight", {
      configurable: true,
      get: () => 600,
    });
  });

  it("renders compact chrome with one Dify-style internal start", async () => {
    renderIteration();

    expect(await screen.findByText("Iteration")).toBeInTheDocument();
    expect(screen.getByText("Region nodes: 2")).toBeInTheDocument();
    expect(
      screen.getByRole("img", { name: "Internal start" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", {
        name: "Add a node to the iteration region",
      }),
    ).toBeInTheDocument();
    expect(screen.queryByText("Hidden in the inspector")).toBeNull();
    expect(
      document.querySelectorAll("[data-workflow-iteration-start]"),
    ).toHaveLength(1);
    expect(
      document.querySelector("[data-workflow-iteration-region]"),
    ).not.toHaveClass("border-dashed");
  });

  it("keeps the start visible but removes editing affordances in read-only mode", async () => {
    renderIteration({ readOnly: true });

    expect(
      await screen.findByRole("img", { name: "Internal start" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", {
        name: "Add a node to the iteration region",
      }),
    ).toBeNull();
  });

  it("folds the internal canvas into the compact header", async () => {
    renderIteration({ collapsed: true });

    expect(await screen.findByText("Region nodes: 2")).toBeInTheDocument();
    expect(screen.queryByRole("img", { name: "Internal start" })).toBeNull();
    expect(
      document.querySelector("[data-workflow-iteration-region]"),
    ).toBeNull();
  });

  it("reveals the entry plus affordance only while hovering the start row", async () => {
    renderIteration();

    expect(await screen.findByRole("img", { name: "Internal start" }));
    const insert = screen.getByRole("button", {
      name: "Add a node to the iteration region",
    });
    expect(insert).toHaveClass(
      "opacity-0",
      "group-hover/iteration-start:opacity-100",
      "focus-visible:opacity-100",
      "data-popup-open:opacity-100",
    );
    const start = document.querySelector("[data-workflow-iteration-start]");
    expect(start).not.toBeNull();
    expect(start!.parentElement).toHaveClass(
      "nodrag",
      "nopan",
      "group/iteration-start",
    );
  });

  it("renders a Dify-style bottom-right resize affordance when editable", async () => {
    renderIteration();

    await screen.findByRole("img", { name: "Internal start" });
    const control = document.querySelector<HTMLElement>(
      ".react-flow__resize-control.handle.bottom.right",
    );
    expect(control).not.toBeNull();
    expect(control).toHaveClass("nodrag");
    expect(control).toHaveStyle({
      right: "6px",
      bottom: "6px",
      width: "20px",
      height: "20px",
    });
    const icon = control!.querySelector("svg");
    expect(icon).not.toBeNull();
    expect(icon).toHaveClass(
      "opacity-0",
      "group-hover/iteration-resize:opacity-100",
    );
  });

  it("hides the resize affordance when collapsed or read-only", async () => {
    renderIteration({ readOnly: true });
    await screen.findByRole("img", { name: "Internal start" });
    expect(document.querySelector(".react-flow__resize-control")).toBeNull();
  });

  it("collapses remove the resize affordance too", async () => {
    renderIteration({ collapsed: true });

    await screen.findByText("Region nodes: 2");
    expect(document.querySelector(".react-flow__resize-control")).toBeNull();
  });

  it("resizes the frame from the bottom-right corner and persists the size", async () => {
    // jsdom performs no layout; React Flow seeds `measured` from the node
    // wrapper's offset size, which the resizer uses as its gesture start size.
    Object.defineProperty(HTMLElement.prototype, "offsetWidth", {
      configurable: true,
      get: () => WORKFLOW_ITERATION_NODE_WIDTH,
    });
    Object.defineProperty(HTMLElement.prototype, "offsetHeight", {
      configurable: true,
      get: () => WORKFLOW_ITERATION_NODE_HEIGHT,
    });
    try {
      renderEditableIteration();
      const control = await waitForResizeControl();
      control.setPointerCapture = () => {};

      await act(async () => {
        control.dispatchEvent(
          windowedMouseEvent("mousedown", {
            button: 0,
            clientX: 600,
            clientY: 380,
          }),
        );
      });
      await act(async () => {
        window.dispatchEvent(
          windowedMouseEvent("mousemove", {
            button: 0,
            clientX: 700,
            clientY: 460,
          }),
        );
      });
      await act(async () => {
        window.dispatchEvent(
          windowedMouseEvent("mouseup", {
            button: 0,
            clientX: 700,
            clientY: 460,
          }),
        );
      });

      expect(
        document.querySelector<HTMLElement>("[data-workflow-iteration-frame]"),
      ).toHaveStyle({
        width: `${WORKFLOW_ITERATION_NODE_WIDTH + 100}px`,
        height: `${WORKFLOW_ITERATION_NODE_HEIGHT + 80}px`,
      });
    } finally {
      Reflect.deleteProperty(HTMLElement.prototype, "offsetWidth");
      Reflect.deleteProperty(HTMLElement.prototype, "offsetHeight");
    }
  });
});

/** Waits for the frame's bottom-right resize control to mount inside React Flow. */
async function waitForResizeControl(): Promise<HTMLElement> {
  await screen.findByRole("img", { name: "Internal start" });
  const control = document.querySelector<HTMLElement>(
    ".react-flow__resize-control.handle.bottom.right",
  );
  if (control === null) {
    throw new Error("iteration resize control did not mount");
  }
  return control;
}
