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

test("projected text and metadata-free child tool deltas never advance the parent observation", async () => {
  let time = 100;
  let markReady = () => {};
  let finish = () => {};
  const ready = new Promise<void>((resolve) => {
    markReady = resolve;
  });
  const completed = new Promise<void>((resolve) => {
    finish = resolve;
  });
  const childMeta = {
    "opencode/child-session": { id: "child", parentID: "root", depth: 1 },
  };
  const client: ChatSessionClient = {
    async *prompt() {
      yield {
        type: "session_update",
        update: {
          sessionUpdate: "agent_message_chunk",
          content: { type: "text", text: "Parent" },
        },
      } satisfies PromptSessionEvent;
      time = 200;
      yield {
        type: "session_update",
        update: {
          sessionUpdate: "agent_message_chunk",
          content: { type: "text", text: "Background" },
          _meta: childMeta,
        },
      } satisfies PromptSessionEvent;
      time = 300;
      yield {
        type: "session_update",
        update: {
          sessionUpdate: "tool_call",
          toolCallId: "child-tool",
          title: "Child tool",
          status: "in_progress",
          _meta: childMeta,
        },
      } satisfies PromptSessionEvent;
      time = 400;
      yield {
        type: "session_update",
        update: {
          sessionUpdate: "tool_call_update",
          toolCallId: "child-tool",
          status: "in_progress",
        },
      } satisfies PromptSessionEvent;
      markReady();
      await completed;
      yield {
        type: "retrying",
        retry: 1,
        maxRetries: 3,
      } satisfies PromptSessionEvent;
      time = 500;
      yield {
        type: "session_update",
        update: {
          sessionUpdate: "tool_call",
          toolCallId: "child-tool",
          title: "Reused by parent",
          status: "in_progress",
        },
      } satisfies PromptSessionEvent;
      yield {
        type: "completed",
        stopReason: "end_turn",
      } satisfies PromptSessionEvent;
    },
    async *load() {
      yield { type: "completed" as const };
    },
    setConfig: async () => ({ configOptions: [] }),
    respondToPermission: async () => ({}),
  };
  const store = createChatStore(client, { now: () => time });
  const sending = store
    .getState()
    .sendMessage({ oraSessionId: "one", text: "go" });
  await ready;
  assert.deepEqual(store.getState().conversations.one!.diagnostics, {
    lastActivity: { kind: "message", receivedAt: 100 },
    lastChildActivity: { kind: "tool", receivedAt: 400 },
    toolSources: { "child-tool": "child" },
  });
  finish();
  await sending;
  assert.deepEqual(store.getState().conversations.one!.diagnostics, {
    lastActivity: { kind: "tool", receivedAt: 500 },
    toolSources: { "child-tool": "parent" },
  });
  store.getState().adoptSwitchedAgent("one", []);
  assert.equal(store.getState().conversations.one!.diagnostics, undefined);
  store.getState().dispose();
});

test("failure and cancellation clear transient reports while a new prompt clears prior observations", async () => {
  for (const ending of ["failure", "cancel"] as const) {
    let first = true;
    const client: ChatSessionClient = {
      async *prompt() {
        if (first) {
          first = false;
          yield {
            type: "session_update",
            update: {
              sessionUpdate: "agent_message_chunk",
              content: { type: "text", text: "Observed" },
            },
          } satisfies PromptSessionEvent;
          yield {
            type: "session_update",
            update: {
              sessionUpdate: "session_info_update",
              _meta: { "opencode/retry": { attempt: 1 } },
            },
          } satisfies PromptSessionEvent;
          if (ending === "failure") throw new Error("transport lost");
          yield {
            type: "completed",
            stopReason: "cancelled",
          } satisfies PromptSessionEvent;
        } else
          yield {
            type: "completed",
            stopReason: "end_turn",
          } satisfies PromptSessionEvent;
      },
      async *load() {
        yield { type: "completed" as const };
      },
      setConfig: async () => ({ configOptions: [] }),
      respondToPermission: async () => ({}),
    };
    const store = createChatStore(client, { now: () => 50 });
    const sending = store
      .getState()
      .sendMessage({ oraSessionId: "one", text: "go" });
    if (ending === "failure") await assert.rejects(sending, /transport lost/);
    else await sending;
    assert.equal(store.getState().conversations.one!.isResponding, false);
    assert.equal(
      store.getState().conversations.one!.diagnostics?.provider,
      undefined,
    );
    assert.deepEqual(
      store.getState().conversations.one!.diagnostics?.lastActivity,
      { kind: "message", receivedAt: 50 },
    );
    await store.getState().sendMessage({ oraSessionId: "one", text: "again" });
    assert.equal(store.getState().conversations.one!.diagnostics, undefined);
    store.getState().dispose();
  }
});
