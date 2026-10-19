import type { Node, NodeChange } from "@xyflow/react";

/** Screen-pixel distance at which a dragged edge snaps to another node. */
export const WORKFLOW_ALIGNMENT_SCREEN_PX = 5;

const GUIDE_PADDING = 12;
const ALIGN_EPSILON = 0.5;

/** A guide drawn in flow coordinates while a drag is aligned with other nodes. */
export interface AlignmentGuide {
  orientation: "horizontal" | "vertical";
  /** `x` for a vertical line, `y` for a horizontal line. */
  position: number;
  start: number;
  end: number;
}

interface AlignBox {
  id: string;
  parentId: string | undefined;
  x: number;
  y: number;
  width: number;
  height: number;
}

interface AxisSnap {
  delta: number;
  target: number;
}

/**
 * Snaps dragged nodes to nearby node edges and centers, and reports the guides
 * for that snap. Positions already on the grid stay put unless another node's
 * edge is within `threshold` flow pixels. A parent and its children share one
 * delta so a child is not shifted twice.
 */
export function applyDragAlignment<TNode extends Node>(
  changes: readonly NodeChange<TNode>[],
  nodes: readonly TNode[],
  threshold: number,
): { changes: readonly NodeChange<TNode>[]; guides: AlignmentGuide[] } {
  const draggedIds = new Set(
    changes.flatMap((change) =>
      change.type === "position" &&
      change.position !== undefined &&
      change.dragging !== undefined
        ? [change.id]
        : [],
    ),
  );
  if (draggedIds.size === 0 || threshold < 0) {
    return { changes, guides: [] };
  }

  const byId = new Map(nodes.map((node) => [node.id, node]));
  const boxes = nodes.flatMap((node) => {
    const box = alignmentBox(node, byId);
    return box === null ? [] : [box];
  });
  const boxById = new Map(boxes.map((box) => [box.id, box]));
  const draggedBoxes = [...draggedIds].flatMap((id) => {
    const box = boxById.get(id);
    return box === undefined ? [] : [box];
  });
  const roots = draggedBoxes.filter(
    (box) => !hasDraggedAncestor(box, draggedIds, byId),
  );
  if (roots.length === 0) {
    return { changes, guides: [] };
  }
  const parentId = roots[0]?.parentId;
  if (roots.some((box) => box.parentId !== parentId)) {
    return { changes, guides: [] };
  }

  const group = unionBox(roots);
  const peers = boxes.filter(
    (box) =>
      box.parentId === parentId &&
      !draggedIds.has(box.id) &&
      !hasDraggedAncestor(box, draggedIds, byId),
  );
  const xSnap = closestSnap(
    xAnchors(group),
    peers.flatMap((peer) => xAnchors(peer)),
    threshold,
  );
  const ySnap = closestSnap(
    yAnchors(group),
    peers.flatMap((peer) => yAnchors(peer)),
    threshold,
  );
  const dx = xSnap?.delta ?? 0;
  const dy = ySnap?.delta ?? 0;
  const guides = [
    ...(xSnap === undefined
      ? []
      : [verticalGuide(group, peers, xSnap.target, dx, dy)]),
    ...(ySnap === undefined
      ? []
      : [horizontalGuide(group, peers, ySnap.target, dx, dy)]),
  ];
  if (dx === 0 && dy === 0) {
    return { changes, guides };
  }

  const rootIds = new Set(roots.map((box) => box.id));
  return {
    changes: changes.map((change) => {
      if (
        change.type !== "position" ||
        change.position === undefined ||
        !rootIds.has(change.id)
      ) {
        return change;
      }
      return {
        ...change,
        position: {
          x: change.position.x + dx,
          y: change.position.y + dy,
        },
      };
    }),
    guides,
  };
}

function alignmentBox(
  node: Node,
  byId: ReadonlyMap<string, Node>,
): AlignBox | null {
  if (node.hidden === true) {
    return null;
  }
  const width = node.measured?.width ?? node.width;
  const height = node.measured?.height ?? node.height;
  if (
    width === undefined ||
    height === undefined ||
    width <= 0 ||
    height <= 0
  ) {
    return null;
  }
  const origin = absoluteOrigin(node, byId);
  return {
    id: node.id,
    parentId: node.parentId,
    x: origin.x,
    y: origin.y,
    width,
    height,
  };
}

function absoluteOrigin(
  node: Node,
  byId: ReadonlyMap<string, Node>,
): { x: number; y: number } {
  let x = node.position.x;
  let y = node.position.y;
  let parentId = node.parentId;
  const seen = new Set<string>();
  while (parentId !== undefined && !seen.has(parentId)) {
    seen.add(parentId);
    const parent = byId.get(parentId);
    if (parent === undefined) {
      break;
    }
    x += parent.position.x;
    y += parent.position.y;
    parentId = parent.parentId;
  }
  return { x, y };
}

function hasDraggedAncestor(
  box: AlignBox,
  draggedIds: ReadonlySet<string>,
  byId: ReadonlyMap<string, Node>,
): boolean {
  let parentId = box.parentId;
  const seen = new Set<string>();
  while (parentId !== undefined && !seen.has(parentId)) {
    if (draggedIds.has(parentId)) {
      return true;
    }
    seen.add(parentId);
    parentId = byId.get(parentId)?.parentId;
  }
  return false;
}

function unionBox(boxes: readonly AlignBox[]): AlignBox {
  const x = Math.min(...boxes.map((box) => box.x));
  const y = Math.min(...boxes.map((box) => box.y));
  const right = Math.max(...boxes.map((box) => box.x + box.width));
  const bottom = Math.max(...boxes.map((box) => box.y + box.height));
  return {
    id: "group",
    parentId: boxes[0]?.parentId,
    x,
    y,
    width: right - x,
    height: bottom - y,
  };
}

function xAnchors(box: AlignBox): number[] {
  return [box.x, box.x + box.width / 2, box.x + box.width];
}

function yAnchors(box: AlignBox): number[] {
  return [box.y, box.y + box.height / 2, box.y + box.height];
}

function closestSnap(
  draggedAnchors: readonly number[],
  peerAnchors: readonly number[],
  threshold: number,
): AxisSnap | undefined {
  let best: (AxisSnap & { distance: number }) | undefined;
  for (const draggedValue of draggedAnchors) {
    for (const target of peerAnchors) {
      const delta = target - draggedValue;
      const distance = Math.abs(delta);
      if (distance > threshold) {
        continue;
      }
      if (
        best === undefined ||
        distance < best.distance - ALIGN_EPSILON ||
        (Math.abs(distance - best.distance) <= ALIGN_EPSILON &&
          Math.abs(delta) < Math.abs(best.delta))
      ) {
        best = { delta, target, distance };
      }
    }
  }
  return best === undefined
    ? undefined
    : { delta: best.delta, target: best.target };
}

function verticalGuide(
  group: AlignBox,
  peers: readonly AlignBox[],
  target: number,
  dx: number,
  dy: number,
): AlignmentGuide {
  const shifted = { ...group, x: group.x + dx, y: group.y + dy };
  const involved = [
    shifted,
    ...peers.filter((peer) =>
      xAnchors(peer).some((value) => Math.abs(value - target) <= ALIGN_EPSILON),
    ),
  ];
  return {
    orientation: "vertical",
    position: target,
    start: Math.min(...involved.map((box) => box.y)) - GUIDE_PADDING,
    end: Math.max(...involved.map((box) => box.y + box.height)) + GUIDE_PADDING,
  };
}

function horizontalGuide(
  group: AlignBox,
  peers: readonly AlignBox[],
  target: number,
  dx: number,
  dy: number,
): AlignmentGuide {
  const shifted = { ...group, x: group.x + dx, y: group.y + dy };
  const involved = [
    shifted,
    ...peers.filter((peer) =>
      yAnchors(peer).some((value) => Math.abs(value - target) <= ALIGN_EPSILON),
    ),
  ];
  return {
    orientation: "horizontal",
    position: target,
    start: Math.min(...involved.map((box) => box.x)) - GUIDE_PADDING,
    end: Math.max(...involved.map((box) => box.x + box.width)) + GUIDE_PADDING,
  };
}
