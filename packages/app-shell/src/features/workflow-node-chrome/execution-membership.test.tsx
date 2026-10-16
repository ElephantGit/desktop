import { memo } from "react";
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { WorkflowMembershipProvider } from "./execution-membership";
import { useWorkflowNodeUnused } from "./execution-membership-context";

const Card = memo(function Card({
  id,
  renders,
}: {
  id: string;
  renders: { current: Record<string, number> };
}) {
  const count = renders.current;
  count[id] = (count[id] ?? 0) + 1;
  const unused = useWorkflowNodeUnused(id);
  return <span data-testid={id}>{unused ? "unused" : "used"}</span>;
});

describe("workflow unused membership", () => {
  it("redraws only the card whose unused flag changed", () => {
    const renders = {
      current: {} as Record<string, number>,
    };
    const view = (ids: string[]) => (
      <WorkflowMembershipProvider unusedNodeIds={ids}>
        <Card id="a" renders={renders} />
        <Card id="b" renders={renders} />
      </WorkflowMembershipProvider>
    );
    const { rerender } = render(view(["a"]));
    expect(screen.getByTestId("a")).toHaveTextContent("unused");
    expect(screen.getByTestId("b")).toHaveTextContent("used");
    const afterMount = { ...renders.current };

    rerender(view(["a"]));
    expect(renders.current).toEqual(afterMount);

    rerender(view(["a", "c"]));
    expect(renders.current).toEqual(afterMount);

    rerender(view(["b"]));
    expect(renders.current.a).toBe(afterMount.a + 1);
    expect(renders.current.b).toBe(afterMount.b + 1);
    expect(screen.getByTestId("a")).toHaveTextContent("used");
    expect(screen.getByTestId("b")).toHaveTextContent("unused");
  });
});
