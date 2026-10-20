import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, it } from "vitest";
import { createChatStore } from "@ora/chat";
import type { LoadSessionEvent, PromptSessionEvent } from "@ora/contracts";
import { createTestClient } from "../../test/contracts-transport";
import { appI18n } from "../../i18n/i18n-instance";
import { ResponseTurn } from "./response-turn";

beforeEach(async () => {
  await act(() => appI18n.changeLanguage("en-US"));
});
const meta = (id: string, parentID: string, depth: number, title: string) => ({
  "opencode/child-session": { id, parentID, depth, title },
});
const worker = meta("worker", "root", 1, "Outer worker");
const nested = meta("nested", "worker", 2, "Nested worker");
const live: PromptSessionEvent[] = [
  {
    type: "session_update",
    update: {
      sessionUpdate: "agent_thought_chunk",
      messageId: "thought",
      content: { type: "text", text: "I will delegate" },
      _meta: worker,
    },
  },
  {
    type: "session_update",
    update: {
      sessionUpdate: "tool_call",
      toolCallId: "nested:shell",
      title: "Probe command",
      kind: "execute",
      status: "completed",
      rawInput: { command: "echo proof" },
      rawOutput: { exit: 0 },
      content: [{ type: "content", content: { type: "text", text: "proof" } }],
      _meta: nested,
    },
  },
  {
    type: "session_update",
    update: {
      sessionUpdate: "agent_message_chunk",
      messageId: "nested-result",
      content: { type: "text", text: "NESTED_RESULT" },
      _meta: nested,
    },
  },
  {
    type: "session_update",
    update: {
      sessionUpdate: "agent_message_chunk",
      messageId: "worker-result",
      content: { type: "text", text: "WORKER_RESULT" },
      _meta: worker,
    },
  },
  {
    type: "session_update",
    update: {
      sessionUpdate: "agent_message_chunk",
      messageId: "parent-result",
      content: { type: "text", text: "PARENT_RESULT" },
    },
  },
  { type: "completed", stopReason: "end_turn" },
];
async function* stream<T>(events: T[]) {
  for (const event of events) yield structuredClone(event);
}

it("renders nested read-only details from the production store and keeps cold replay attribution", async () => {
  const cold: LoadSessionEvent[] = [
    {
      type: "session_update",
      update: {
        sessionUpdate: "user_message_chunk",
        content: { type: "text", text: "go" },
      },
    },
    ...live.flatMap((event): LoadSessionEvent[] =>
      event.type === "session_update" ? [event] : [],
    ),
    { type: "turn_ended", stopReason: "end_turn" },
    { type: "completed" },
  ];
  const client = createTestClient({
    promptSession: () => stream(live),
    loadSession: () => stream(cold),
  });
  const store = createChatStore(client.session, { now: () => 100 });
  await store.getState().sendMessage({ oraSessionId: "live", text: "go" });
  await store.getState().loadSession("cold");
  const summary = (id: string) =>
    store
      .getState()
      .conversations[id]!.turns[0]!.items.map((item) => ({
        kind: item.kind,
        metadata: item.metadata,
        content: "content" in item ? item.content : undefined,
      }));
  expect(summary("cold")).toEqual(summary("live"));
  const user = userEvent.setup();
  render(
    <ResponseTurn
      turn={store.getState().conversations.cold!.turns[0]!}
      userName="Reader"
    />,
  );
  const outer = screen.getByRole("region", { name: "Subagent · Outer worker" });
  await user.click(
    within(outer).getByRole("button", { name: /Subagent · Outer worker/ }),
  );
  const child = screen.getByRole("region", {
    name: "Subagent · Nested worker",
  });
  await user.click(
    within(child).getByRole("button", { name: /Subagent · Nested worker/ }),
  );
  expect(within(child).getByText("NESTED_RESULT")).toBeInTheDocument();
  expect(within(outer).getByText("WORKER_RESULT")).toBeInTheDocument();
  expect(within(outer).queryByText("PARENT_RESULT")).not.toBeInTheDocument();
  expect(screen.getAllByText("Status unknown")).toHaveLength(2);
  expect(
    screen.queryByRole("button", { name: /steer|cancel child|close child/i }),
  ).not.toBeInTheDocument();
  store.getState().dispose();
});
