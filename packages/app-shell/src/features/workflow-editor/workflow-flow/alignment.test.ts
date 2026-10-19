import { describe, expect, it } from "vitest";
import type { Node, NodeChange } from "@xyflow/react";

import { applyDragAlignment } from "./alignment";

function node(
  id: string,
  x: number,
  y: number,
  width = 100,
  height = 40,
  parentId?: string,
): Node {
  return {
    id,
    position: { x, y },
    width,
    height,
    parentId,
    data: {},
  };
}

function move(
  id: string,
  x: number,
  y: number,
  dragging: boolean | undefined = true,
): NodeChange {
  return { id, type: "position", position: { x, y }, dragging };
}

describe("applyDragAlignment", () => {
  it("snaps a nearby top edge and draws a horizontal guide", () => {
    const dragged = node("a", 0, 10);
    const peer = node("b", 200, 14);
    const result = applyDragAlignment([move("a", 0, 10)], [dragged, peer], 5);

    expect(result.changes[0]).toMatchObject({
      position: { x: 0, y: 14 },
    });
    expect(result.guides).toEqual([
      expect.objectContaining({
        orientation: "horizontal",
        position: 14,
      }),
    ]);
  });

  it("keeps a grid position when no other node is close", () => {
    const changes = [move("a", 0, 10)];
    const result = applyDragAlignment(
      changes,
      [node("a", 0, 10), node("b", 80, 80)],
      5,
    );

    expect(result.changes).toBe(changes);
    expect(result.guides).toEqual([]);
  });

  it("draws a vertical guide when left edges already match", () => {
    const result = applyDragAlignment(
      [move("a", 20, 0)],
      [node("a", 20, 0), node("b", 20, 120)],
      5,
    );

    expect(result.changes[0]).toMatchObject({ position: { x: 20, y: 0 } });
    expect(result.guides).toEqual([
      expect.objectContaining({ orientation: "vertical", position: 20 }),
    ]);
  });

  it("aligns an iteration child to its sibling in flow coordinates", () => {
    const parent = node("frame", 100, 50, 400, 300);
    const sibling = node("sibling", 10, 10, 80, 40, "frame");
    const dragged = node("child", 14, 80, 80, 40, "frame");
    const result = applyDragAlignment(
      [move("child", 14, 80)],
      [parent, sibling, dragged],
      5,
    );

    expect(result.changes[0]).toMatchObject({
      position: { x: 10, y: 80 },
    });
    expect(result.guides).toEqual([
      expect.objectContaining({ orientation: "vertical", position: 110 }),
    ]);
  });

  it("does not snap a dragged frame onto its own child", () => {
    const parent = node("frame", 20, 0, 400, 300);
    const child = node("child", 0, 0, 80, 40, "frame");
    const changes = [move("frame", 20, 0), move("child", 0, 0)];
    const result = applyDragAlignment(changes, [parent, child], 5);

    expect(result.changes).toBe(changes);
    expect(result.guides).toEqual([]);
  });

  it("shifts every selected sibling by the same snap", () => {
    const result = applyDragAlignment(
      [move("a", 0, 0), move("b", 0, 80)],
      [node("a", 0, 0), node("b", 0, 80), node("peer", 8, 200)],
      10,
    );

    expect(result.changes[0]).toMatchObject({ position: { x: 8, y: 0 } });
    expect(result.changes[1]).toMatchObject({ position: { x: 8, y: 80 } });
  });

  it("ignores a hidden node", () => {
    const changes = [move("a", 0, 0)];
    const hidden = node("b", 4, 0);
    hidden.hidden = true;
    const result = applyDragAlignment(changes, [node("a", 0, 0), hidden], 5);

    expect(result.changes).toBe(changes);
    expect(result.guides).toEqual([]);
  });
});
