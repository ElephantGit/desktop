import { childSessionSource, type ChatTurnItem } from "@ora/chat";

/** One explicitly reported source with a cycle-safe nested view of the same timeline. */
export interface SubagentGroupItem {
  kind: "subagentGroup";
  id: string;
  sourceId: string;
  parentId: string;
  depth: number;
  index: number;
  title?: string;
  relationshipUnknown: boolean;
  items: ChatTurnItem[];
  children: SubagentGroupItem[];
  timeline: (ChatTurnItem | SubagentGroupItem)[];
}

/** Groups explicit ids only; incomplete/conflicting relationships remain readable flat groups. */
export function groupSubagentItems(
  items: ChatTurnItem[],
): (ChatTurnItem | SubagentGroupItem)[] {
  const groups = new Map<string, SubagentGroupItem>();
  for (const item of items) {
    const source = childSessionSource(item.metadata);
    if (!source) continue;
    let group = groups.get(source.id);
    if (!group) {
      group = {
        kind: "subagentGroup",
        id: `subagent:${source.id}`,
        sourceId: source.id,
        parentId: source.parentId,
        depth: source.depth,
        index: groups.size + 1,
        title: source.title,
        relationshipUnknown: false,
        items: [],
        children: [],
        timeline: [],
      };
      groups.set(source.id, group);
    } else if (
      group.parentId !== source.parentId ||
      group.depth !== source.depth
    ) {
      group.relationshipUnknown = true;
    }
    group.title = source.title ?? group.title;
    group.items.push(item);
  }
  const parents = new Map<string, SubagentGroupItem>();
  for (const group of groups.values()) {
    const seen = new Set([group.sourceId]);
    let ancestor = groups.get(group.parentId);
    while (ancestor) {
      if (
        seen.has(ancestor.sourceId) ||
        seen.size >= 32 ||
        ancestor.relationshipUnknown
      ) {
        group.relationshipUnknown = true;
        break;
      }
      seen.add(ancestor.sourceId);
      ancestor = groups.get(ancestor.parentId);
    }
    const parent = groups.get(group.parentId);
    if (!parent && group.depth > 1) group.relationshipUnknown = true;
    if (parent && !group.relationshipUnknown) {
      parents.set(group.sourceId, parent);
      parent.children.push(group);
    }
  }
  const result: (ChatTurnItem | SubagentGroupItem)[] = [];
  const emitted = new Set<string>();
  for (const item of items) {
    const source = childSessionSource(item.metadata);
    if (!source) {
      result.push(item);
      continue;
    }
    let group = groups.get(source.id)!;
    group.timeline.push(item);
    while (parents.has(group.sourceId)) {
      const parent = parents.get(group.sourceId)!;
      if (!parent.timeline.includes(group)) parent.timeline.push(group);
      group = parent;
    }
    if (!emitted.has(group.sourceId)) {
      result.push(group);
      emitted.add(group.sourceId);
    }
  }
  return result;
}
