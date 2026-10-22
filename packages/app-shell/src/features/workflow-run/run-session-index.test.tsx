import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { expect, it } from "vitest";
import type { GetSessionBindingsResponse } from "@ora/contracts";
import { createChatStore } from "@ora/chat";
import { appI18n } from "../../i18n/i18n-instance";
import { createTestClient } from "../../test/contracts-transport";
import {
  createHookWrapper,
  createTestQueryClient,
} from "../../test/hook-harness";
import { RunSessionIndex } from "./run-session-index";

it("maps reversed replies by session id and keeps executions without a session", async () => {
  await act(() => appI18n.changeLanguage("en-US"));
  const calls: string[][] = [];
  const client = createTestClient({
    getSessionBindings: ({ sessionIds }) => {
      calls.push(sessionIds);
      return {
        sessions: [...sessionIds].reverse().map((sessionId) => ({
          type: "available" as const,
          sessionId,
          history: "readable" as const,
          segments: [
            {
              key: "live",
              agentRef: "uninstalled.plugin",
              agentSessionId: `provider-for-${sessionId}`,
              recordedAt: null,
              current: true,
            },
          ],
        })),
      };
    },
  });
  const wrapper = createHookWrapper(
    client,
    createTestQueryClient(),
    createChatStore(client.session),
  );
  const executions = [
    {
      key: "one",
      nodeId: "node-one",
      scopeId: "round-one",
      iteration: 0,
      status: "succeeded",
      sessionId: "a",
    },
    {
      key: "two",
      nodeId: "node-two",
      scopeId: "round-two",
      iteration: 1,
      status: "failed",
      sessionId: "b",
    },
    { key: "none", nodeId: "node-none", scopeId: "root", status: "failed" },
  ];
  render(<RunSessionIndex runId="run" executions={executions} />, { wrapper });
  const user = userEvent.setup();
  await user.click(
    screen.getByRole("button", { name: appI18n.t("chat.bindingRunIndex") }),
  );
  await screen.findByText("provider-for-a");
  expect(calls).toEqual([["a", "b"]]);
  const rows = screen.getAllByRole("row").slice(1);
  expect(rows).toHaveLength(3);
  for (const [index, id] of ["a", "b"].entries()) {
    const row = within(rows[index]!);
    expect(row.getByText(`provider-for-${id}`)).toBeInTheDocument();
    expect(
      row.queryByText(`provider-for-${id === "a" ? "b" : "a"}`),
    ).not.toBeInTheDocument();
    await user.click(row.getByRole("button", { name: id }));
    const dialogs = screen.getAllByRole("dialog");
    const panel = within(dialogs[dialogs.length - 1]!);
    expect(await panel.findByText("Ora Session ID")).toBeInTheDocument();
    expect(panel.getByText(id, { selector: "code" })).toBeInTheDocument();
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.getAllByRole("dialog")).toHaveLength(1));
  }
  const absent = within(rows[2]!);
  expect(
    absent.getByText(appI18n.t("chat.bindingNoSession")),
  ).toBeInTheDocument();
  expect(
    absent.getByText(appI18n.t("workflowRun.status.failed")),
  ).toBeInTheDocument();
  expect(absent.queryByRole("button")).not.toBeInTheDocument();
});

it.each(["run", "round"])(
  "isolates late replies after changing %s and reopening the index",
  async (scope) => {
    await act(() => appI18n.changeLanguage("en-US"));
    const requests: Array<{
      ids: string[];
      signal?: AbortSignal;
      resolve: (response: GetSessionBindingsResponse) => void;
    }> = [];
    const client = createTestClient({
      getSessionBindings: ({ sessionIds }, options) =>
        new Promise((resolve) => {
          requests.push({ ids: sessionIds, signal: options?.signal, resolve });
        }),
    });
    const wrapper = createHookWrapper(
      client,
      createTestQueryClient(),
      createChatStore(client.session),
    );
    const execution = (sessionId: string) => [
      {
        key: sessionId,
        nodeId: `node-${sessionId}`,
        scopeId: "root",
        status: "succeeded",
        sessionId,
      },
    ];
    const view = render(
      <RunSessionIndex runId="old-run" executions={execution("old")} />,
      { wrapper },
    );
    const user = userEvent.setup();
    await user.click(
      screen.getByRole("button", { name: appI18n.t("chat.bindingRunIndex") }),
    );
    await waitFor(() => expect(requests).toHaveLength(1));
    view.rerender(
      <RunSessionIndex
        runId={scope === "run" ? "new-run" : "old-run"}
        executions={execution("new")}
      />,
    );
    await waitFor(() => expect(requests).toHaveLength(2));
    expect(requests[0]!.signal?.aborted).toBe(true);
    await act(async () =>
      requests[0]!.resolve({
        sessions: [{ type: "not_found", sessionId: "old" }],
      }),
    );
    expect(
      screen.queryByText(appI18n.t("chat.bindingNotFound")),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("node-old")).not.toBeInTheDocument();
    await user.keyboard("{Escape}");
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(requests[1]!.signal?.aborted).toBe(true);
    await user.click(
      screen.getByRole("button", { name: appI18n.t("chat.bindingRunIndex") }),
    );
    await waitFor(() => expect(requests).toHaveLength(3));
    await act(async () =>
      requests[1]!.resolve({
        sessions: [{ type: "not_found", sessionId: "new" }],
      }),
    );
    expect(
      screen.queryByText(appI18n.t("chat.bindingNotFound")),
    ).not.toBeInTheDocument();
    await act(async () =>
      requests[2]!.resolve({
        sessions: [{ type: "unavailable", sessionId: "new" }],
      }),
    );
    expect(
      await screen.findByText(appI18n.t("chat.bindingUnavailable")),
    ).toBeInTheDocument();
    expect(requests.map(({ ids }) => ids)).toEqual([["old"], ["new"], ["new"]]);
  },
);

it("renders three executions, fetches shared sessions once and retains missing sessions", async () => {
  await act(() => appI18n.changeLanguage("en-US"));
  const calls: string[][] = [];
  const client = createTestClient({
    getSessionBindings: ({ sessionIds }) => {
      calls.push(sessionIds);
      return {
        sessions: sessionIds.map((sessionId) => ({
          type: "not_found" as const,
          sessionId,
        })),
      };
    },
  });
  const wrapper = createHookWrapper(
    client,
    createTestQueryClient(),
    createChatStore(client.session),
  );
  const executions = [
    {
      key: "a",
      nodeId: "node-a",
      scopeId: "root",
      status: "succeeded",
      sessionId: "shared",
    },
    {
      key: "b",
      nodeId: "node-b",
      scopeId: "root",
      status: "succeeded",
      sessionId: "shared",
    },
    {
      key: "a-first",
      nodeId: "node-a",
      scopeId: "root",
      status: "failed",
      sessionId: "deleted",
      attempt: 1,
    },
  ];
  render(<RunSessionIndex runId="run" executions={executions} />, { wrapper });
  expect(calls).toEqual([]);
  const user = userEvent.setup();
  await user.click(
    screen.getByRole("button", { name: appI18n.t("chat.bindingRunIndex") }),
  );
  await waitFor(() => expect(calls).toEqual([["deleted", "shared"]]));
  expect(screen.getAllByRole("row")).toHaveLength(4);
  const summaries = await screen.findAllByText(
    appI18n.t("chat.bindingTimeline"),
  );
  await user.click(summaries[2]!);
  expect(screen.getAllByText(appI18n.t("chat.bindingNotFound"))).toHaveLength(
    3,
  );
  await user.click(screen.getByRole("button", { name: "deleted" }));
  expect(await screen.findByText("Ora Session ID")).toBeInTheDocument();
  expect(screen.getAllByTestId("run-diagnostics")).toHaveLength(1);
});
