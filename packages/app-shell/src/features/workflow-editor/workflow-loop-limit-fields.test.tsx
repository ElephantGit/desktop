import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it } from "vitest";
import {
  createMockWorkflowCapabilities,
  createMockWorkflowLoopGroup,
  type WorkflowNodeData,
} from "@ora/workflow-mock";
import {
  parseWorkflowGraph,
  serializeWorkflowGraph,
} from "@ora/workflow-runtime";
import type { Node } from "@xyflow/react";
import { appI18n } from "../../i18n/i18n-instance";
import { AppI18nProvider } from "../../i18n/i18n";
import { WorkflowInspector } from "./workflow-inspector";

describe("Loop round limit policy", () => {
  it.each(["new", "legacy"])(
    "edits and persists the %s loop policy through the inspector",
    async (kind) => {
      await appI18n.changeLanguage("zh-CN");
      const user = userEvent.setup();
      const group = createMockWorkflowLoopGroup({
        sequence: 1,
        position: { x: 0, y: 0 },
        locale: "zh-CN",
      });
      const loop = group.nodes[0]!;
      if (kind === "legacy") delete loop.data.loopConfig!.maxIterationsBehavior;
      let latest = loop;
      function Harness({
        initial = loop,
      }: {
        initial?: Node<WorkflowNodeData, "workflow">;
      }) {
        const [current, setCurrent] = useState(initial);
        return (
          <AppI18nProvider>
            <WorkflowInspector
              node={current}
              graphNodes={group.nodes.map((node) =>
                node.id === current.id ? current : node,
              )}
              variableCatalog={[]}
              capabilities={createMockWorkflowCapabilities("zh-CN")}
              onUpdate={(next) => {
                latest = next;
                setCurrent(next);
              }}
              onDelete={() => undefined}
              onCloseNode={() => undefined}
            />
          </AppI18nProvider>
        );
      }
      const view = render(<Harness />);
      const toggle = screen.getByRole("switch", {
        name: "达到最大轮次时中断工作流",
      });
      expect(toggle).toBeChecked();
      await user.click(toggle);
      expect(toggle).not.toBeChecked();
      expect(latest.data.loopConfig).toEqual({
        ...loop.data.loopConfig,
        maxIterationsBehavior: "succeed",
      });
      const saved = serializeWorkflowGraph({
        nodes: group.nodes.map((node) =>
          node.id === latest.id ? latest : node,
        ),
        edges: group.edges.map(({ id, source, target }) => ({
          id,
          source,
          target,
        })),
        viewport: { x: 0, y: 0, zoom: 1 },
      });
      const restored = parseWorkflowGraph(saved).nodes.find(
        (node) => node.id === loop.id,
      )!;
      expect(restored.data.loopConfig).toEqual(latest.data.loopConfig);
      view.unmount();
      render(
        <Harness initial={restored as Node<WorkflowNodeData, "workflow">} />,
      );
      const reopened = screen.getByRole("switch", {
        name: "达到最大轮次时中断工作流",
      });
      expect(reopened).not.toBeChecked();
      await user.click(reopened);
      expect(reopened).toBeChecked();
      expect(latest.data.loopConfig).toEqual({
        ...loop.data.loopConfig,
        maxIterationsBehavior: "fail",
      });
    },
  );
});
