import { createElement, type ReactNode } from "react";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { locateSessionChild } from "./session-child-navigation";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ChatTurn } from "@ora/chat";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { AppI18nProvider } from "../../i18n/i18n";
import "../../i18n/i18n-instance";
import { ContractsClientContext } from "../../contracts-client-context";
import { ChatStoreContext } from "../../chat-store-context";
import { createChatStore } from "@ora/chat";
import {
  createTestClient,
  type TestHandlers,
} from "../../test/contracts-transport";
import { MessageList } from "./message-list";
import { MESSAGE_LIST_VIRTUALIZE_MIN_ROWS } from "./message-list-rows";

/**
 * Gives the row virtualizer a viewport size in jsdom. jsdom performs no
 * layout, so every element reports zero offsetHeight and the window would
 * compute as empty; @tanstack/react-virtual reads offsetHeight for its rect.
 */
function mockViewportSize(height: number) {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(
    height,
  );
  vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(
    height,
  );
  vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(800);
}

function completedTurn(index: number): ChatTurn {
  return {
    id: `turn-${index}`,
    userMessage: {
      kind: "message",
      id: `turn-${index}-user`,
      role: "user",
      content: `prompt ${index}`,
      createdAt: index,
    },
    items: [
      {
        kind: "message",
        id: `turn-${index}-assistant`,
        role: "assistant",
        content: `answer ${index}`,
        createdAt: index,
      },
    ],
    status: "completed",
    stopReason: null,
    error: null,
    createdAt: index,
  };
}

function renderList(turns: ChatTurn[]) {
  const client = createTestClient({} satisfies TestHandlers);
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, staleTime: 0 } },
  });
  const chatStore = createChatStore(client.session);
  const wrapper = ({ children }: { children: ReactNode }) =>
    createElement(
      QueryClientProvider,
      { client: queryClient },
      createElement(
        ContractsClientContext.Provider,
        { value: client },
        createElement(
          ChatStoreContext.Provider,
          { value: chatStore },
          createElement(AppI18nProvider, null, children),
        ),
      ),
    );
  return render(
    <main data-session-transcript="session">
      <MessageList turns={turns} userName="Eric" isResponding={false} />
    </main>,
    { wrapper },
  );
}

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("MessageList virtualization", () => {
  it("keeps a located child in view when deferred layout resizes the transcript", async () => {
    let resize: ResizeObserverCallback | undefined;
    vi.stubGlobal(
      "ResizeObserver",
      class {
        constructor(callback: ResizeObserverCallback) {
          resize = callback;
        }
        observe() {}
        unobserve() {}
        disconnect() {}
      },
    );
    const child = completedTurn(1);
    child.items[0]!.metadata = {
      "opencode/child-session": { id: "child", parentID: "root", depth: 1 },
    };
    renderList([child, completedTurn(2)]);
    const scroller = screen.getByTestId("message-list");
    Object.defineProperty(scroller, "scrollHeight", { value: 3000 });
    scroller.scrollTop = 2500;
    act(() => {
      locateSessionChild("session", "turn-1:display:subagent:child");
    });
    await waitFor(() =>
      expect(
        document.querySelector('[data-subagent-group="child"] button'),
      ).toHaveFocus(),
    );
    scroller.scrollTop = 120;
    act(() => resize?.([], {} as ResizeObserver));
    expect(scroller.scrollTop).toBe(120);
    await userEvent.setup().keyboard("{Enter}");
    expect(screen.getByText("answer 1")).toBeVisible();
  });
  it("locates an offscreen child in the owning session without duplicating its tree", async () => {
    mockViewportSize(480);
    vi.spyOn(HTMLElement.prototype, "scrollHeight", "get").mockReturnValue(
      100_000,
    );
    const turns = Array.from({ length: 40 }, (_, index) =>
      completedTurn(index + 1),
    );
    turns[39]!.items[0]!.metadata = {
      "opencode/child-session": {
        id: "child",
        parentID: "root",
        depth: 1,
        title: "Offscreen worker",
      },
    };
    renderList(turns);
    const scroller = screen.getByTestId("message-list");
    Object.defineProperty(scroller, "scrollTo", {
      configurable: true,
      value: (options: ScrollToOptions) => {
        scroller.scrollTop = options.top ?? 0;
        scroller.dispatchEvent(new Event("scroll"));
      },
    });
    expect(document.querySelector('[data-subagent-group="child"]')).toBeNull();
    expect(
      locateSessionChild("other-session", "turn-40:display:subagent:child"),
    ).toBe(false);
    act(() => {
      expect(
        locateSessionChild("session", "turn-40:display:subagent:child"),
      ).toBe(true);
    });
    await waitFor(() =>
      expect(
        document.querySelector('[data-subagent-group="child"] button'),
      ).toHaveFocus(),
    );
    expect(
      document.querySelectorAll('[data-subagent-group="child"]'),
    ).toHaveLength(1);
  });
  it("keeps a short thread fully mounted without a measured viewport", () => {
    renderList([completedTurn(1), completedTurn(2)]);
    expect(screen.getByText("prompt 1")).toBeInTheDocument();
    expect(screen.getByText("prompt 2")).toBeInTheDocument();
    expect(document.querySelectorAll("[data-index]")).toHaveLength(0);
  });

  it("mounts only a window of rows once the thread exceeds the threshold", () => {
    mockViewportSize(480);
    const turns = Array.from({ length: 40 }, (_item, index) =>
      completedTurn(index + 1),
    );
    expect(turns.length * 3 + 1).toBeGreaterThan(
      MESSAGE_LIST_VIRTUALIZE_MIN_ROWS,
    );
    renderList(turns);

    expect(screen.getByText("prompt 1")).toBeInTheDocument();
    expect(screen.queryByText("prompt 40")).not.toBeInTheDocument();
    const mounted = document.querySelectorAll("[data-index]").length;
    expect(mounted).toBeGreaterThan(0);
    expect(mounted).toBeLessThan(turns.length);
  });
});
