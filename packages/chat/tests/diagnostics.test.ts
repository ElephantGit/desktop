import assert from "node:assert/strict";
import test from "node:test";
import type { PromptSessionEvent } from "@ora/contracts";
import { createChatStore, type ChatSessionClient } from "../src/index.ts";
import { observeDiagnostics } from "../src/diagnostics.ts";

test("only recognized provider reports establish state and null clears retries", () => {
  assert.equal(
    observeDiagnostics(
      undefined,
      { sessionUpdate: "session_info_update", _meta: { status: "healthy" } },
      100,
    ),
    undefined,
  );
  const retry = observeDiagnostics(
    undefined,
    {
      sessionUpdate: "session_info_update",
      _meta: { "opencode/retry": { attempt: 2 } },
    },
    100,
  );
  assert.deepEqual(retry, { provider: { kind: "retry", attempt: 2 } });
  assert.deepEqual(
    observeDiagnostics(
      retry,
      {
        sessionUpdate: "session_info_update",
        _meta: { "opencode/retry": null },
      },
      101,
    ),
    { provider: undefined },
  );
});

test("live observation clears on reload and remains isolated from another session", async () => {
  const client: ChatSessionClient = {
    async *prompt() {
      yield {
        type: "session_update",
        update: {
          sessionUpdate: "agent_message_chunk",
          content: { type: "text", text: "hello" },
        },
      } satisfies PromptSessionEvent;
      yield {
        type: "completed",
        stopReason: "end_turn",
      } satisfies PromptSessionEvent;
    },
    async *load() {
      yield {
        type: "session_update" as const,
        update: {
          sessionUpdate: "agent_message_chunk" as const,
          content: { type: "text" as const, text: "old reply" },
        },
        recordedAt: "2026-10-19T12:00:00+08:00",
      };
      yield { type: "completed" as const };
    },
    setConfig: async () => ({ configOptions: [] }),
    respondToPermission: async () => ({}),
  };
  const store = createChatStore(client, { now: () => 123456 });
  store.getState().initializeSession("other");
  await store.getState().sendMessage({ oraSessionId: "one", text: "hi" });
  assert.deepEqual(store.getState().conversations.one?.diagnostics, {
    lastActivity: { kind: "message", receivedAt: 123456 },
  });
  assert.equal(store.getState().conversations.other?.diagnostics, undefined);
  await store.getState().loadSession("one");
  assert.equal(store.getState().conversations.one?.diagnostics, undefined);
  store.getState().dispose();
});
