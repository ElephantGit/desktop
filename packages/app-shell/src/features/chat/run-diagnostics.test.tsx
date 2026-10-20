import { act, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { appI18n } from "../../i18n/i18n-instance";
import { RunDiagnosticsPanel } from "./run-diagnostics";

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
        screen.getByText(/No new activity for 10 seconds/),
      ).toBeInTheDocument();
      act(() => vi.advanceTimersByTime(5000));
      expect(
        screen.getByText(/No new activity for 15 seconds/),
      ).toBeInTheDocument();
      view.unmount();
      expect(vi.getTimerCount()).toBe(0);
    } finally {
      vi.useRealTimers();
    }
  });
});
