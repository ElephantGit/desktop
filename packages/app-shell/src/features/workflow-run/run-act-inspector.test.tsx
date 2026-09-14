import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { createChatStore } from "@ora/chat";
import {
  createTestClient,
  type TestHandlers,
} from "../../test/contracts-transport";
import { createPluginMemory, pluginHandlers } from "../../test/memory/plugins";
import { createAgentMemory, agentHandlers } from "../../test/memory/agents";
import { createSkillMemory, skillHandlers } from "../../test/memory/skills";
import {
  createHookWrapper,
  createTestQueryClient,
} from "../../test/hook-harness";
import { appI18n } from "../../i18n/i18n-instance";
import { RunActInspector } from "./run-act-inspector";
import type {
  GraphWorkflowNodeState,
  WorkflowNodeData,
} from "@ora/workflow-runtime";
import { AGENT_REF } from "../../test/agent-identity";

/** State for this test surface; no unrelated domain fixtures are initialized. */
function createFixtureState() {
  return {
    ...createPluginMemory(),
    ...createAgentMemory(),
    ...createSkillMemory(),
  };
}

type FixtureState = ReturnType<typeof createFixtureState>;

/** Explicit domain composition for the behaviors exercised by this test file. */
function createFixtureHandlers(state: FixtureState): TestHandlers {
  return {
    ...pluginHandlers(state),
    ...agentHandlers(state),
    ...skillHandlers(state),
  };
}

const AGENT_DATA: WorkflowNodeData = {
  kind: "agent",
  title: "探索",
  description: "只读探索项目现状",
  agentConfig: {
    schemaVersion: 3,
    executor: {
      agentCli: AGENT_REF.opencode,
      modelId: "deepseek/deepseek-v4-pro",
    },
    roleId: "研究员",
    skills: [
      { skillId: "openspec-explore", enabled: true },
      { skillId: "hidden-skill", enabled: false },
    ],
    mcps: [],
    prompt: "阅读相关代码并输出风险。",
  },
};

/** Mounts the act inspector with catalog-backed Agent/Skill names. */
function renderInspector(
  nodeState: GraphWorkflowNodeState = { status: "succeeded" },
) {
  const state = createFixtureState();
  state.agents = [
    {
      id: "ag-researcher",
      namespace: "local",
      name: "研究员",
      description: "只读探索项目现状和影响范围",
    },
  ];
  state.skills = [
    {
      id: "sk-explore",
      namespace: "local",
      name: "openspec-explore",
      description: "探索仓库结构与约束",
      source: { kind: "local" } as const,
      availability: "available",
    },
    {
      id: "sk-disabled",
      namespace: "local",
      name: "hidden-skill",
      description: "Should not appear",
      source: { kind: "local" } as const,
      availability: "available",
    },
  ];
  const clientHandlers: TestHandlers = createFixtureHandlers(state);
  const client = createTestClient(clientHandlers);
  const queryClient = createTestQueryClient();
  const Wrapper = createHookWrapper(
    client,
    queryClient,
    createChatStore(client.session),
  );

  return {
    user: userEvent.setup(),
    ...render(
      <Wrapper>
        <RunActInspector
          nodeId="agent-1"
          data={AGENT_DATA}
          state={nodeState}
          artifacts={[]}
          revealedArtifactId={null}
          onClose={() => undefined}
        />
      </Wrapper>,
    ),
  };
}

describe("RunActInspector agent config", () => {
  it("shows read-only agent fields and skill briefs for enabled skills only", async () => {
    await appI18n.changeLanguage("zh-CN");
    const { user } = renderInspector();

    await waitFor(() => {
      expect(
        screen.getByText("OpenCode · deepseek/deepseek-v4-pro"),
      ).toBeInTheDocument();
      expect(
        screen.getByRole("button", { name: "查看角色「研究员」简介" }),
      ).toBeInTheDocument();
      expect(
        screen.getByRole("button", {
          name: "查看 Skill「openspec-explore」简介",
        }),
      ).toBeInTheDocument();
    });
    expect(screen.getByText("阅读相关代码并输出风险。")).toBeInTheDocument();
    expect(screen.queryByText("hidden-skill")).not.toBeInTheDocument();
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();

    await user.click(
      screen.getByRole("button", { name: "查看角色「研究员」简介" }),
    );
    expect(
      await screen.findByText("只读探索项目现状和影响范围"),
    ).toBeInTheDocument();

    await user.click(
      screen.getByRole("button", {
        name: "查看 Skill「openspec-explore」简介",
      }),
    );
    expect(await screen.findByText("探索仓库结构与约束")).toBeInTheDocument();
  });
});

describe("RunActInspector failure detail", () => {
  it("renders the kind title, hint, and attempt line for a failed node", async () => {
    await appI18n.changeLanguage("zh-CN");
    renderInspector({
      status: "failed",
      errorMessage: "agent node review structured output failed: not json",
      errorDetail: {
        kind: "structured_output",
        message: "agent node review structured output failed: not json",
        sourceChain: ["not json"],
        attempt: 2,
        resumable: false,
        recordedAt: 50,
      },
    });

    expect(await screen.findByText("结构化输出不合格")).toBeInTheDocument();
    expect(
      screen.getByText(
        "智能体的回复不符合输出结构，调整提示词或输出结构后发布新版本",
      ),
    ).toBeInTheDocument();
    expect(screen.getByText("第 2 次尝试")).toBeInTheDocument();
    expect(
      screen.getByText(
        "这类失败通常源于工作流本身，直接续跑很可能再次失败；建议修改工作流后重新运行。",
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText("agent node review structured output failed: not json"),
    ).toBeInTheDocument();
  });

  it("omits the not-resumable hint when the failure is resumable", async () => {
    await appI18n.changeLanguage("zh-CN");
    renderInspector({
      status: "failed",
      errorMessage: '{"reason":"interrupted_by_restart"}',
      errorDetail: {
        kind: "interrupted_by_restart",
        message: '{"reason":"interrupted_by_restart"}',
        sourceChain: [],
        attempt: 1,
        resumable: true,
        recordedAt: 80,
      },
    });

    expect(await screen.findByText("被应用重启打断")).toBeInTheDocument();
    expect(
      screen.getByText("应用重启时该节点仍在运行，可直接续跑"),
    ).toBeInTheDocument();
    expect(screen.getByText("第 1 次尝试")).toBeInTheDocument();
    expect(
      screen.queryByText(
        "这类失败通常源于工作流本身，直接续跑很可能再次失败；建议修改工作流后重新运行。",
      ),
    ).not.toBeInTheDocument();
  });
});
