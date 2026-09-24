import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it } from "vitest";
import {
  createMockWorkflowCapabilities,
  createMockWorkflowLoopGroup,
  type WorkflowNodeData,
} from "@ora/workflow-mock";
import type { Node } from "@xyflow/react";
import { appI18n } from "../../i18n/i18n-instance";
import { AppI18nProvider } from "../../i18n/i18n";
import { WorkflowInspector } from "./workflow-inspector";

describe("Loop end conditions", () => {
  it("edits typed rules and logic, preserves the loop, and limits selection to visible values", async () => {
    await appI18n.changeLanguage("zh-CN");
    const user = userEvent.setup();
    const group = createMockWorkflowLoopGroup({
      sequence: 1,
      position: { x: 0, y: 0 },
      locale: "zh-CN",
    });
    const loop = group.nodes[0]!;
    const agent = group.nodes.find((node) => node.data.kind === "agent")!;
    agent.data.agentConfig!.outputContract = {
      type: "structured",
      schema: {
        type: "object",
        properties: { passed: { type: "boolean" }, score: { type: "number" } },
        required: ["passed", "score"],
        additionalProperties: false,
      },
    };
    const unrelated = { ...agent, id: "unrelated", parentId: undefined };
    let latest = loop;
    function Harness() {
      const [current, setCurrent] =
        useState<Node<WorkflowNodeData, "workflow">>(loop);
      return (
        <AppI18nProvider>
          <WorkflowInspector
            node={current}
            graphNodes={[...group.nodes, unrelated]}
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
    render(<Harness />);
    expect(
      screen.getByRole("region", { name: "结束条件" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "移除条件" })).toBeDisabled();
    expect(screen.queryByLabelText("值 1")).not.toBeInTheDocument();
    await user.click(screen.getByLabelText("变量 1"));
    expect(
      screen.queryByRole("option", { name: "unrelated.output" }),
    ).not.toBeInTheDocument();
    expect(
      await screen.findByRole("option", { name: `${loop.id}.value` }),
    ).toBeInTheDocument();
    await user.click(
      await screen.findByRole("option", {
        name: `${agent.id}.structured_output.passed`,
      }),
    );
    await user.click(screen.getByLabelText("条件 1"));
    expect(
      screen.queryByRole("option", { name: "包含" }),
    ).not.toBeInTheDocument();
    await user.click(await screen.findByRole("option", { name: "等于" }));
    await user.click(screen.getByLabelText("值 1"));
    await user.click(await screen.findByRole("option", { name: "true" }));
    await user.click(screen.getByRole("button", { name: "添加条件" }));
    await user.click(screen.getByLabelText("变量 2"));
    await user.click(
      await screen.findByRole("option", {
        name: `${agent.id}.structured_output.score`,
      }),
    );
    await user.click(screen.getByLabelText("条件 2"));
    await user.click(await screen.findByRole("option", { name: "大于等于" }));
    await user.type(screen.getByLabelText("值 2"), "90");
    await user.click(screen.getByLabelText("结束条件组合方式 · 1"));
    await user.click(await screen.findByRole("option", { name: /OR/ }));
    await waitFor(() =>
      expect(latest.data.loopConfig).toEqual({
        ...loop.data.loopConfig,
        until: {
          logic: "or",
          conditions: [
            {
              variableSelector: [agent.id, "structured_output", "passed"],
              operator: "equals",
              value: true,
            },
            {
              variableSelector: [agent.id, "structured_output", "score"],
              operator: "greater_than_or_equal",
              value: 90,
            },
          ],
        },
      }),
    );
    await user.click(screen.getAllByRole("button", { name: "移除条件" })[1]!);
    expect(screen.getByRole("button", { name: "移除条件" })).toBeDisabled();
    expect(latest.data.loopConfig?.until.conditions).toEqual([
      {
        variableSelector: [agent.id, "structured_output", "passed"],
        operator: "equals",
        value: true,
      },
    ]);
    await user.click(screen.getByLabelText("变量 1"));
    await user.click(
      await screen.findByRole("option", { name: `${agent.id}.output` }),
    );
    expect(latest.data.loopConfig?.until.conditions).toEqual([
      {
        variableSelector: [agent.id, "output"],
        operator: "",
        value: undefined,
      },
    ]);
    await user.click(screen.getByLabelText("条件 1"));
    expect(
      screen.queryByRole("option", { name: "大于等于" }),
    ).not.toBeInTheDocument();
    await user.click(await screen.findByRole("option", { name: "包含" }));
    await user.type(screen.getByLabelText("值 1"), "true");
    expect(latest.data.loopConfig?.until.conditions).toEqual([
      {
        variableSelector: [agent.id, "output"],
        operator: "contains",
        value: "true",
      },
    ]);
  });
});
