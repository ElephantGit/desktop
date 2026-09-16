import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createMockWorkflowCapabilities } from "@ora/workflow-mock";
import { AppI18nProvider } from "../../../i18n/i18n";
import { appI18n } from "../../../i18n/i18n-instance";
import {
  IterationInsertMenu,
  WorkflowIterationActionsProvider,
} from "./iteration-actions";

describe("iteration insert actions", () => {
  beforeEach(async () => {
    await appI18n.changeLanguage("en-US");
  });

  it("offers only capability-declared iteration node types", async () => {
    const user = userEvent.setup();
    const onInsert = vi.fn();
    render(
      <AppI18nProvider>
        <WorkflowIterationActionsProvider
          capabilities={createMockWorkflowCapabilities("en-US")}
          nodes={[]}
          edges={[]}
          readOnly={false}
          onInsert={onInsert}
          onToggleCollapsed={vi.fn()}
        >
          <IterationInsertMenu
            insertion={{ type: "entry", iterationId: "iter" }}
            label="Add iteration node"
          />
        </WorkflowIterationActionsProvider>
      </AppI18nProvider>,
    );

    await user.click(
      screen.getByRole("button", { name: "Add iteration node" }),
    );
    expect(
      await screen.findByRole("menuitem", { name: "Agent" }),
    ).toBeVisible();
    expect(screen.getByRole("menuitem", { name: "Condition" })).toBeVisible();
    expect(screen.queryByRole("menuitem", { name: "Start" })).toBeNull();

    await user.click(screen.getByRole("menuitem", { name: "Agent" }));
    expect(onInsert).toHaveBeenCalledWith("agent", {
      type: "entry",
      iterationId: "iter",
    });
  });
});
