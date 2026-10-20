import type { ChatItemMetadata, ChatToolCall } from "./types.ts";

function object(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

/** Canonicalizes JSON objects so wire key order cannot change live/replay merge boundaries. */
function canonical(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(canonical);
  if (object(value))
    return Object.fromEntries(
      Object.keys(value)
        .sort()
        .filter((key) => value[key] !== undefined)
        .map((key) => [key, canonical(value[key])]),
    );
  return value;
}

/** Full metadata and text annotations define a merge boundary, not merely a child id. */
export function metadataKey(
  metadata: unknown,
  context?: ChatItemMetadata["contentContext"],
): string | undefined {
  if (
    !object(metadata) &&
    context?.metadata === undefined &&
    context?.annotations === undefined
  )
    return undefined;
  return JSON.stringify(
    canonical([
      metadata ?? null,
      context?.metadata ?? null,
      context?.annotations ?? null,
    ]),
  );
}

export interface ChildSessionSource {
  id: string;
  parentId: string;
  depth: number;
  title?: string;
}

/** Parses only OpenCode's explicit projection; names, tool prefixes and depth do not infer edges. */
export function childSessionSource(
  metadata: unknown,
): ChildSessionSource | undefined {
  if (!object(metadata)) return undefined;
  const child = metadata["opencode/child-session"];
  if (
    !object(child) ||
    typeof child.id !== "string" ||
    !child.id.trim() ||
    typeof child.parentID !== "string" ||
    !child.parentID.trim() ||
    typeof child.depth !== "number" ||
    !Number.isSafeInteger(child.depth) ||
    child.depth < 1
  )
    return undefined;
  return {
    id: child.id,
    parentId: child.parentID,
    depth: child.depth,
    ...(typeof child.title === "string" && child.title.trim()
      ? { title: child.title }
      : {}),
  };
}

/** Retains metadata without interpreting or recursively merging unknown provider values. */
export function withItemMetadata<Item extends ChatItemMetadata>(
  item: Item,
  metadata: unknown,
  contentContext?: ChatItemMetadata["contentContext"],
): Item {
  return {
    ...item,
    ...(object(metadata) ? { metadata } : {}),
    ...(contentContext?.metadata !== undefined ||
    contentContext?.annotations !== undefined
      ? { contentContext }
      : {}),
  };
}

export function mergeToolMetadata(
  previous: Record<string, unknown> | undefined,
  incoming: unknown,
): Record<string, unknown> | undefined {
  return object(incoming) ? { ...previous, ...incoming } : previous;
}

export function withToolMetadata(
  tool: ChatToolCall,
  metadata: unknown,
): ChatToolCall {
  return withItemMetadata(tool, metadata);
}
