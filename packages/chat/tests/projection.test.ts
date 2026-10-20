import assert from "node:assert/strict";
import test from "node:test";
import type * as acp from "@agentclientprotocol/sdk";
import type { LoadSessionEvent, PromptSessionEvent } from "@ora/contracts";
import {
  childSessionSource,
  createChatStore,
  type ChatSessionClient,
} from "../src/index.ts";

const source = (id: string) => ({
  "opencode/child-session": { id, parentID: "root", depth: 1 },
});
const text = (
  id: string,
  body: string,
  messageId?: string,
): acp.SessionUpdate => ({
  sessionUpdate: "agent_message_chunk",
  ...(messageId ? { messageId } : {}),
  content: { type: "text", text: body, _meta: { format: "plain" } },
  _meta: source(id),
});

async function* events<T>(items: T[]) {
  yield* items;
}

test("live batching and reload keep colliding child text ids and annotations distinct", async () => {
  const updates = [
    text("left", "A", "shared"),
    text("right", "B", "shared"),
    text("left", "C", "shared"),
  ];
  const client: ChatSessionClient = {
    prompt: () =>
      events<PromptSessionEvent>([
        ...updates.map((update) => ({
          type: "session_update" as const,
          update,
        })),
        { type: "completed", stopReason: "end_turn" },
      ]),
    load: () =>
      events<LoadSessionEvent>([
        {
          type: "session_update",
          update: {
            sessionUpdate: "user_message_chunk",
            content: { type: "text", text: "go" },
          },
        },
        ...[text("left", "AC", "shared"), text("right", "B", "shared")].map(
          (update) => ({ type: "session_update" as const, update }),
        ),
        { type: "turn_ended", stopReason: "end_turn" },
        { type: "completed" },
      ]),
    setConfig: async () => ({ configOptions: [] }),
    respondToPermission: async () => ({}),
  };
  const store = createChatStore(client, { now: () => 42 });
  await store.getState().sendMessage({ oraSessionId: "live", text: "go" });
  await store.getState().loadSession("cold");
  const summary = (id: string) =>
    store
      .getState()
      .conversations[id]!.turns[0]!.items.map((item) => ({
        kind: item.kind,
        metadata: item.metadata,
        context: item.contentContext,
        text: item.kind === "message" ? item.content : "",
      }));
  assert.deepEqual(summary("live"), summary("cold"));
  assert.deepEqual(
    summary("live").map((x) => x.text),
    ["AC", "B"],
  );
  store.getState().dispose();
});

test("late attribution, opaque replacement and unknown child outcomes survive finalization", async () => {
  const client: ChatSessionClient = {
    async *prompt() {
      yield {
        type: "session_update",
        update: {
          sessionUpdate: "tool_call",
          toolCallId: "t",
          title: "Work",
          status: "in_progress",
        },
      } satisfies PromptSessionEvent;
      yield {
        type: "session_update",
        update: {
          sessionUpdate: "tool_call_update",
          toolCallId: "t",
          _meta: { ...source("child"), other: { stale: true } },
        },
      } satisfies PromptSessionEvent;
      yield {
        type: "session_update",
        update: {
          sessionUpdate: "tool_call_update",
          toolCallId: "t",
          _meta: { other: { fresh: true } },
        },
      } satisfies PromptSessionEvent;
      yield {
        type: "completed",
        stopReason: "end_turn",
      } satisfies PromptSessionEvent;
    },
    load: () => events<LoadSessionEvent>([]),
    setConfig: async () => ({ configOptions: [] }),
    respondToPermission: async () => ({}),
  };
  const store = createChatStore(client);
  await store.getState().sendMessage({ oraSessionId: "one", text: "go" });
  const item = store.getState().conversations.one!.turns[0]!.items[0]!;
  assert.equal(item.kind, "toolCall");
  assert.deepEqual(item.metadata, {
    ...source("child"),
    other: { fresh: true },
  });
  if (item.kind === "toolCall") assert.equal(item.status, "unknown");
  assert.equal(
    childSessionSource({ "opencode/child-session": null }),
    undefined,
  );
  store.getState().dispose();
});
