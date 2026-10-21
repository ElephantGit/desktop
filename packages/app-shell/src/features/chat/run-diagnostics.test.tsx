import { act, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { appI18n } from "../../i18n/i18n-instance";
import { RunDiagnosticsPanel } from "./run-diagnostics";
import type { ChatToolCall, ChatTurn } from "@ora/chat";

beforeEach(async () => {
  await act(() => appI18n.changeLanguage("en-US"));
});

describe("run diagnostics", () => {
  it("does not invent provider health or turn replay into live activity", () => {
    render(
      <RunDiagnosticsPanel
        turns={[]}
        isResponding={false}
        pendingPermissions={[]}
      />,
    );
    expect(screen.getByText("Not reported (unknown)")).toBeInTheDocument();
    expect(
      screen.getByText("Unknown; replay is not new activity"),
    ).toBeInTheDocument();
    expect(screen.queryByText(/healthy|stalled/i)).not.toBeInTheDocument();
  });

  it("says no activity was observed yet instead of calling a fresh attempt a replay", () => {
    render(
      <RunDiagnosticsPanel turns={[]} isResponding pendingPermissions={[]} />,
    );
    expect(
      screen.getByText("No activity observed in this attempt yet"),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("Unknown; replay is not new activity"),
    ).not.toBeInTheDocument();
  });

  it("reports elapsed silence without creating activity and releases its timer", () => {
    vi.useFakeTimers();
    try {
      vi.setSystemTime(100_000);
      const view = render(
        <RunDiagnosticsPanel
          diagnostics={{ lastActivity: { kind: "tool", receivedAt: 90_000 } }}
          turns={[]}
          isResponding
          pendingPermissions={[]}
        />,
      );
      expect(
        screen.getByText(/No new parent activity for 10 seconds/),
      ).toBeInTheDocument();
      act(() => vi.advanceTimersByTime(5000));
      expect(
        screen.getByText(/No new parent activity for 15 seconds/),
      ).toBeInTheDocument();
      view.unmount();
      expect(vi.getTimerCount()).toBe(0);
    } finally {
      vi.useRealTimers();
    }
  });

  it("labels parent, child and unobserved tools separately", () => {
    const tool = (id: string, title: string): ChatToolCall => ({
      kind: "toolCall",
      id,
      title,
      status: "in_progress",
      content: [],
      locations: [],
      createdAt: 1,
      updatedAt: 1,
    });
    const turn: ChatTurn = {
      id: "turn",
      userMessage: {
        kind: "message",
        id: "user",
        role: "user",
        content: "go",
        createdAt: 1,
      },
      items: [
        tool("parent", "Parent delegate"),
        tool("child", "Background worker"),
        tool("old", "Historical tool"),
      ],
      status: "streaming",
      stopReason: null,
      error: null,
      createdAt: 1,
    };
    const view = render(
      <RunDiagnosticsPanel
        diagnostics={{
          lastActivity: { kind: "message", receivedAt: 100 },
          lastChildActivity: { kind: "tool", receivedAt: 200 },
          toolSources: { parent: "parent", child: "child" },
        }}
        turns={[turn]}
        isResponding
        pendingPermissions={[]}
      />,
    );
    expect(
      screen.getByText("Parent tools").nextElementSibling,
    ).toHaveTextContent("Parent delegate");
    expect(
      screen.getByText("Parent tools").nextElementSibling,
    ).not.toHaveTextContent("Background worker");
    expect(
      screen.getByText("Child tools").nextElementSibling,
    ).toHaveTextContent("Background worker");
    expect(
      screen.getByText("Tools with unconfirmed source").nextElementSibling,
    ).toHaveTextContent("Historical tool");
    expect(screen.getByText("Latest child activity")).toBeInTheDocument();
    expect(
      screen.getByText(/Automatically handled permissions/),
    ).toBeInTheDocument();
    view.unmount();
  });
});
