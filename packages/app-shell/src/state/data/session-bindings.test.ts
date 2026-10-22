import { act, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { GetSessionBindingsResponse } from "@ora/contracts";
import { appI18n } from "../../i18n/i18n-instance";
import { createTestClient } from "../../test/contracts-transport";
import { renderHookWithClient } from "../../test/hook-harness";
import { useSessionBindingBatch } from "./session-bindings";

void appI18n;
describe("run binding queries", () => {
  it("deduplicates requests, caps serial batches and preserves other results on failure", async () => {
    const calls: string[][] = [];
    const client = createTestClient({
      getSessionBindings: ({ sessionIds }) => {
        calls.push(sessionIds);
        if (calls.length === 2) throw new Error("one batch failed");
        return {
          sessions: sessionIds.map((sessionId) => ({
            type: "not_found" as const,
            sessionId,
          })),
        };
      },
    });
    const ids = Array.from({ length: 65 }, (_, n) => `s${n}`);
    const hook = renderHookWithClient(
      () => useSessionBindingBatch("run", [...ids, ...ids]),
      client,
    );
    await waitFor(() => expect(hook.result.current.isSuccess).toBe(true));
    expect(calls.map((batch) => batch.length)).toEqual([32, 32, 1]);
    expect(
      hook.result.current.data?.filter((item) => item.type === "unavailable"),
    ).toHaveLength(32);
    expect(hook.result.current.data).toHaveLength(65);
    hook.unmount();
    hook.queryClient.clear();
  });
  it("does not issue the next batch after its index closes", async () => {
    let release!: (response: GetSessionBindingsResponse) => void;
    let signal: AbortSignal | undefined;
    let calls = 0;
    const client = createTestClient({
      getSessionBindings: (_, options) => {
        calls++;
        signal = options?.signal;
        return new Promise((resolve) => {
          release = resolve;
        });
      },
    });
    const hook = renderHookWithClient(
      () =>
        useSessionBindingBatch(
          "run",
          Array.from({ length: 40 }, (_, n) => String(n)),
        ),
      client,
    );
    await waitFor(() => expect(calls).toBe(1));
    hook.unmount();
    expect(signal?.aborted).toBe(true);
    await act(async () => {
      release({ sessions: [] });
    });
    expect(calls).toBe(1);
    hook.queryClient.clear();
  });
});
