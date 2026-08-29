import {
  createPlugin,
  createTraceClient,
  HostRequestError,
  type JsonValue,
} from "../src/mod.ts";
import {
  decodeFrames,
  encodeFrame,
  type PluginTransport,
} from "../src/protocol.ts";

/** Compares JSON-compatible values without a Node compatibility dependency. */
function assertEquals(actual: unknown, expected: unknown): void {
  const actualJson = JSON.stringify(actual);
  const expectedJson = JSON.stringify(expected);
  if (actualJson !== expectedJson) {
    throw new Error(`Expected ${expectedJson}, received ${actualJson}`);
  }
}

/** Creates paired in-memory streams for exercising the SDK without global stdio. */
function createTransportHarness(): {
  transport: PluginTransport;
  send: (message: JsonValue) => Promise<void>;
  responses: AsyncGenerator<unknown>;
} {
  const hostInput = new TransformStream<Uint8Array>();
  const pluginOutput = new TransformStream<Uint8Array>(
    undefined,
    undefined,
    new CountQueuingStrategy({ highWaterMark: Infinity }),
  );
  const inputWriter = hostInput.writable.getWriter();
  return {
    transport: {
      readable: hostInput.readable,
      writable: pluginOutput.writable,
      redirectConsole: false,
    },
    send: (message) => inputWriter.write(encodeFrame(message)),
    responses: decodeFrames(pluginOutput.readable),
  };
}

Deno.test(
  "trace calls become ora/session/trace_* requests carrying the surface envelope",
  async () => {
    const plugin = createPlugin();
    const trace = createTraceClient(plugin);
    const harness = createTransportHarness();
    const run = plugin.run(harness.transport);
    await harness.responses.next();

    const surface = { instanceId: 3, generation: 7 };
    const stat = trace.stat(surface, {
      agent: "official/acme",
      sessionId: "s1",
    });
    const first = await harness.responses.next();
    const statRequest = first.value as {
      id: number;
      method: string;
      params: Record<string, unknown>;
    };
    assertEquals(statRequest.method, "ora/session/trace_stat");
    assertEquals(statRequest.params, {
      surface,
      agent: "official/acme",
      sessionId: "s1",
    });
    await harness.send({
      jsonrpc: "2.0",
      id: statRequest.id,
      result: {
        format: "claude_code",
        exists: true,
        sizeBytes: 42,
        mtimeMs: 9,
      },
    });
    assertEquals(await stat, {
      format: "claude_code",
      exists: true,
      sizeBytes: 42,
      mtimeMs: 9,
    });

    const read = trace.read(surface, 10, 4096, undefined, undefined);
    const readRequest = (await harness.responses.next()).value as {
      id: number;
      method: string;
      params: Record<string, unknown>;
    };
    assertEquals(readRequest.method, "ora/session/trace_read");
    assertEquals(readRequest.params, { surface, offset: 10, maxBytes: 4096 });
    await harness.send({
      jsonrpc: "2.0",
      id: readRequest.id,
      result: { text: "abc", nextOffset: 13, done: true },
    });
    assertEquals(await read, { text: "abc", nextOffset: 13, done: true });

    const list = trace.list(surface, "official/acme");
    const listRequest = (await harness.responses.next()).value as {
      id: number;
      method: string;
      params: Record<string, unknown>;
    };
    assertEquals(listRequest.method, "ora/session/trace_list");
    assertEquals(listRequest.params, { surface, agent: "official/acme" });
    await harness.send({
      jsonrpc: "2.0",
      id: listRequest.id,
      result: {
        entries: [{
          sessionId: "s1",
          agent: "official/acme",
          name: null,
          mtimeMs: 1,
          sizeBytes: 2,
        }],
      },
    });
    assertEquals(await list, [
      {
        sessionId: "s1",
        agent: "official/acme",
        name: null,
        mtimeMs: 1,
        sizeBytes: 2,
      },
    ]);

    const agents = trace.agents(surface);
    const agentsRequest = (await harness.responses.next()).value as {
      id: number;
      method: string;
      params: Record<string, unknown>;
    };
    assertEquals(agentsRequest.method, "ora/session/trace_agents");
    assertEquals(agentsRequest.params, { surface });
    await harness.send({
      jsonrpc: "2.0",
      id: agentsRequest.id,
      result: { agents: [{ agent: "official/acme", format: "claude_code" }] },
    });
    assertEquals(await agents, [{
      agent: "official/acme",
      format: "claude_code",
    }]);

    await harness.send({ jsonrpc: "2.0", method: "ora/shutdown" });
    await run;
  },
);

Deno.test("readAll loops chunks by nextOffset until done", async () => {
  const plugin = createPlugin();
  const trace = createTraceClient(plugin);
  const harness = createTransportHarness();
  const run = plugin.run(harness.transport);
  await harness.responses.next();

  const surface = { instanceId: 1, generation: 1 };
  const readAll = trace.readAll(surface, {
    agent: "official/acme",
    sessionId: "s2",
  });

  for (
    const [index, chunk] of [
      { text: "line1\n", nextOffset: 6, done: false },
      { text: "line2\n", nextOffset: 12, done: true },
    ].entries()
  ) {
    const request = (await harness.responses.next()).value as {
      id: number;
      params: Record<string, unknown>;
    };
    assertEquals(request.params, {
      surface,
      agent: "official/acme",
      sessionId: "s2",
      offset: index === 0 ? 0 : 6,
      maxBytes: 1024 * 1024,
    });
    await harness.send({ jsonrpc: "2.0", id: request.id, result: chunk });
  }
  assertEquals(await readAll, "line1\nline2\n");

  await harness.send({ jsonrpc: "2.0", method: "ora/shutdown" });
  await run;
});

Deno.test("host trace errors surface as HostRequestError with the reported kind", async () => {
  const plugin = createPlugin();
  const trace = createTraceClient(plugin);
  const harness = createTransportHarness();
  const run = plugin.run(harness.transport);
  await harness.responses.next();

  const stat = trace.stat({ instanceId: 1, generation: 1 });
  const request = (await harness.responses.next()).value as { id: number };
  await harness.send({
    jsonrpc: "2.0",
    id: request.id,
    error: {
      code: -32004,
      message: "this surface is not bound to a session",
      data: { kind: "session_not_bound" },
    },
  });

  try {
    await stat;
    throw new Error("expected the stat call to fail");
  } catch (error) {
    if (!(error instanceof HostRequestError)) throw error;
    assertEquals(error.kind, "session_not_bound");
    assertEquals(error.code, -32004);
  }

  await harness.send({ jsonrpc: "2.0", method: "ora/shutdown" });
  await run;
});
