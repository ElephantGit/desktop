import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import {
  useWorkflowEdgeHover,
  WorkflowEdgeHoverProvider,
} from "./workflow-edge-hover";

function HoverProbe() {
  const { hoveredEdgeId, reveal, concealSoon } = useWorkflowEdgeHover();
  return (
    <div>
      <output data-testid="hovered">{hoveredEdgeId ?? "none"}</output>
      <button type="button" onMouseEnter={() => reveal("e1")}>
        enter
      </button>
      <button type="button" onMouseLeave={concealSoon}>
        leave
      </button>
    </div>
  );
}

describe("workflow edge hover", () => {
  it("reveals a canvas edge id while the pointer is over it", async () => {
    const user = userEvent.setup();
    render(
      <WorkflowEdgeHoverProvider>
        <HoverProbe />
      </WorkflowEdgeHoverProvider>,
    );
    expect(screen.getByTestId("hovered")).toHaveTextContent("none");
    await user.hover(screen.getByRole("button", { name: "enter" }));
    expect(screen.getByTestId("hovered")).toHaveTextContent("e1");
  });
});
