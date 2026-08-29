import type { Plugin } from "./plugin.ts";
import type { JsonValue } from "./protocol.ts";

const TRACE_STAT = "ora/session/trace_stat";
const TRACE_READ = "ora/session/trace_read";
const TRACE_LIST = "ora/session/trace_list";
const TRACE_AGENTS = "ora/session/trace_agents";

/** `data.kind` values the trace host reports with its errors. */
export const TRACE_ERROR_KINDS = {
  capabilityDenied: "capability_denied",
  surfaceMismatch: "surface_mismatch",
  sessionNotBound: "session_not_bound",
  traceUnavailable: "trace_unavailable",
  invalidParams: "invalid_params",
} as const;

/** The surface envelope of a workbench call, passed through to the host verbatim. */
export type TraceSurface = {
  instanceId: number;
  generation: number;
};

/** A session the host listed; the plugin may read it without a surface binding. */
export type NamedSession = {
  agent: string;
  sessionId: string;
};

/** Metadata of one bound or named session's trace. */
export interface TraceStat {
  format: string;
  exists: boolean;
  sizeBytes: number;
  mtimeMs: number;
}

/** One byte-offset chunk of a trace; `done` ends the read. */
export interface TraceChunk {
  text: string;
  nextOffset: number;
  done: boolean;
}

/** One entry of the host-scanned session listing. */
export interface TraceEntry {
  sessionId: string;
  agent: string;
  name?: string | null;
  mtimeMs: number;
  sizeBytes: number;
}

/** One agent the host holds a trace declaration for, with its format identifier. */
export interface TraceAgent {
  agent: string;
  format: string;
}

/** The default per-request read size, aligned with the host's 1 MiB chunking. */
export const DEFAULT_TRACE_CHUNK_BYTES = 1024 * 1024;

/**
 * The client for the host's `session.trace` capability: byte-offset reads of
 * agent session traces, the host-scanned listing, and the declaration registry.
 *
 * Mirrors {@link createStorage}: the host performs every file access; the plugin
 * never sees a path. A call may be rejected with a {@link HostRequestError}
 * whose `kind` is one of {@link TRACE_ERROR_KINDS}.
 */
export interface TraceCapability {
  /** Metadata of the bound session, or of a listing-named session. */
  stat(surface: TraceSurface, named?: NamedSession): Promise<TraceStat>;
  /** One chunk starting at `offset`; `childSessionId` reads a child of the bound session. */
  read(
    surface: TraceSurface,
    offset: number,
    maxBytes?: number,
    childSessionId?: string,
    named?: NamedSession,
  ): Promise<TraceChunk>;
  /** The host-scanned session listing, optionally filtered to one agent. */
  list(surface: TraceSurface, agent?: string): Promise<TraceEntry[]>;
  /** Reads a whole trace through repeated chunks (for growing files, until `done`). */
  readAll(surface: TraceSurface, named?: NamedSession): Promise<string>;
  /** Every agent with a registered trace declaration, with its format. */
  agents(surface: TraceSurface): Promise<TraceAgent[]>;
}

/** Builds the trace client on top of a plugin's host-request channel. */
export function createTraceClient(plugin: Plugin): TraceCapability {
  return {
    async stat(surface, named) {
      const result = await plugin.request(
        TRACE_STAT,
        traceParams(surface, named),
      );
      if (
        !isRecord(result) ||
        typeof result.format !== "string" ||
        typeof result.exists !== "boolean"
      ) {
        throw new Error(`${TRACE_STAT} returned an invalid result`);
      }
      return result as unknown as TraceStat;
    },
    async read(surface, offset, maxBytes, childSessionId, named) {
      const params: Record<string, JsonValue> = {
        ...traceParams(surface, named),
        offset,
        maxBytes: maxBytes ?? DEFAULT_TRACE_CHUNK_BYTES,
      };
      if (childSessionId !== undefined) {
        params.childSessionId = childSessionId;
      }
      const result = await plugin.request(TRACE_READ, params);
      if (
        !isRecord(result) ||
        typeof result.text !== "string" ||
        typeof result.nextOffset !== "number" ||
        typeof result.done !== "boolean"
      ) {
        throw new Error(`${TRACE_READ} returned an invalid result`);
      }
      return result as unknown as TraceChunk;
    },
    async list(surface, agent) {
      const params: Record<string, JsonValue> = { surface };
      if (agent !== undefined) {
        params.agent = agent;
      }
      const result = await plugin.request(TRACE_LIST, params);
      if (!isRecord(result) || !Array.isArray(result.entries)) {
        throw new Error(`${TRACE_LIST} returned an invalid result`);
      }
      return result.entries as unknown as TraceEntry[];
    },
    async readAll(surface, named) {
      let text = "";
      let offset = 0;
      for (;;) {
        const chunk = await this.read(
          surface,
          offset,
          DEFAULT_TRACE_CHUNK_BYTES,
          undefined,
          named,
        );
        text += chunk.text;
        offset = chunk.nextOffset;
        if (chunk.done) {
          return text;
        }
      }
    },
    async agents(surface) {
      const result = await plugin.request(TRACE_AGENTS, { surface });
      if (!isRecord(result) || !Array.isArray(result.agents)) {
        throw new Error(`${TRACE_AGENTS} returned an invalid result`);
      }
      return result.agents as unknown as TraceAgent[];
    },
  };
}

/** The params shared by stat/read: the surface envelope plus the optional named session. */
function traceParams(
  surface: TraceSurface,
  named?: NamedSession,
): Record<string, JsonValue> {
  const params: Record<string, JsonValue> = { surface };
  if (named !== undefined) {
    params.agent = named.agent;
    params.sessionId = named.sessionId;
  }
  return params;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
