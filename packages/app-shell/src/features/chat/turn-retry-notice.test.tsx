import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { ChatTurn } from "@ora/chat";
import { AppI18nProvider } from "../../i18n/i18n";
import { TurnEnding } from "./response-turn";

/** Creates one response turn that the backend has re-sent once after a stall. */
function retriedTurn(status: ChatTurn["status"]): ChatTurn {
  return {
    id: "turn-1",
    userMessage: {
      kind: "message",
      id: "user-1",
      role: "user",
      content: "Make the change",
      createdAt: 1,
    },
    items: [],
    status,
    stopReason: status === "completed" ? "end_turn" : null,
    error: null,
    createdAt: 1,
    retry: { retry: 2, maxRetries: 3 },
  };
}

describe("turn retry notice", () => {
  it("counts the retry while the re-sent prompt is still streaming", () => {
    render(
      <AppI18nProvider>
        <TurnEnding turn={retriedTurn("streaming")} />
      </AppI18nProvider>,
    );

    // Locale-independent: only the interpolated `retry / maxRetries` is asserted.
    expect(screen.getByRole("status")).toHaveTextContent(/2\/3/);
  });

  it("steps aside once the retried turn has settled", () => {
    render(
      <AppI18nProvider>
        <TurnEnding turn={retriedTurn("completed")} />
      </AppI18nProvider>,
    );

    expect(screen.queryByRole("status")).toBeNull();
    expect(screen.queryByText(/2\/3/)).toBeNull();
  });
});
