import { describe, expect, it } from "vitest";
import type { ChatTurnItem } from "@ora/chat";
import { groupSubagentItems } from "./subagent-items";

const item = (id: string, parentID: string, depth: number): ChatTurnItem => ({
  kind: "message",
  id: `message-${id}`,
  role: "assistant",
  content: id,
  createdAt: 0,
  metadata: { "opencode/child-session": { id, parentID, depth } },
});

describe("explicit child projection tree", () => {
  it("keeps concurrent siblings separate instead of nesting by depth", () => {
    expect(
      groupSubagentItems([item("left", "root", 1), item("right", "root", 1)]),
    ).toMatchObject([
      { sourceId: "left", children: [] },
      { sourceId: "right", children: [] },
    ]);
  });
  it("connects a late parent and keeps unrelated root content", () => {
    const root: ChatTurnItem = {
      kind: "message",
      id: "root",
      role: "assistant",
      content: "parent",
      createdAt: 0,
    };
    const groups = groupSubagentItems([
      root,
      item("grandchild", "child", 2),
      item("child", "root", 1),
    ]);
    expect(groups).toHaveLength(2);
    expect(groups[0]).toEqual(root);
    expect(groups[1]).toMatchObject({
      sourceId: "child",
      children: [{ sourceId: "grandchild" }],
    });
  });

  it("keeps cycles, missing parents and invalid sources readable", () => {
    const groups = groupSubagentItems([
      item("a", "b", 2),
      item("b", "a", 1),
      item("orphan", "missing", 2),
    ]);
    expect(groups).toHaveLength(3);
    for (const group of groups)
      expect(group).toMatchObject({ relationshipUnknown: true, children: [] });
    const invalid = item("bad", "root", 0);
    expect(groupSubagentItems([invalid])).toEqual([invalid]);
  });
});
