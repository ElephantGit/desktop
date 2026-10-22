import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { expect, it } from "vitest";
import { createChatStore } from "@ora/chat";
import { appI18n } from "../../i18n/i18n-instance";
import { createTestClient } from "../../test/contracts-transport";
import {
  createHookWrapper,
  createTestQueryClient,
} from "../../test/hook-harness";
import { RunSessionIndex } from "./run-session-index";

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
