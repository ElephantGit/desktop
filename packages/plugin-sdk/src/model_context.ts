import { PluginMethodError } from "./plugin.ts";
import type {
  AgentModelApplied,
  AgentModelContext,
  JsonValue,
} from "./protocol/index.ts";
import { INVALID_PARAMS } from "./protocol/index.ts";

/** Rejects malformed sensitive startup data without echoing any received value. */
export function parseModelContext(input: JsonValue): AgentModelContext {
  const value = record(input);
  const configuration = record(value.configuration);
  const settingsJson = string(configuration.settingsJson);
  if (settingsJson.length > 256 * 1024) fail();
  let parsed: JsonValue;
  try {
    parsed = JSON.parse(settingsJson);
  } catch {
    fail();
  }
  const settings = record(parsed);
  const model = record(configuration.model);
  const protocol = string(configuration.protocol);
  if (protocol !== "openai-completions" && protocol !== "anthropic-messages") {
    fail();
  }
  if (configuration.schemaVersion !== 2 || configuration.adapterVersion !== 1) {
    fail();
  }
  for (const setting of Object.values(settings)) {
    if (
      !(typeof setting === "string" || typeof setting === "boolean" ||
        (typeof setting === "number" && Number.isFinite(setting)))
    ) fail();
  }
  const fingerprint = string(configuration.declarationFingerprint);
  const digest = string(configuration.packageSha256);
  if (!/^[a-f0-9]{64}$/.test(fingerprint) || !/^[a-f0-9]{64}$/.test(digest)) {
    fail();
  }
  return {
    configuration: {
      pluginId: string(configuration.pluginId),
      pluginVersion: string(configuration.pluginVersion),
      target: string(configuration.target),
      packageSha256: digest,
      schemaVersion: 2,
      declarationFingerprint: fingerprint,
      adapterVersion: 1,
      configurationRevision: integer(configuration.configurationRevision, 0),
      settingsJson,
      protocol,
      model: {
        id: string(model.id),
        name: string(model.name),
        contextWindow: integer(model.contextWindow, 1),
        maxTokens: integer(model.maxTokens, 1),
      },
    },
    proxyBaseUrl: string(value.proxyBaseUrl),
    token: string(value.token),
  };
}

/** Requires proof from the adapter; an older SDK or ignored context never becomes ready. */
export function verifyModelApplied(
  context: AgentModelContext,
  result: AgentModelApplied | void,
): AgentModelApplied {
  const frozen = context.configuration;
  if (
    !result || result.adapterVersion !== frozen.adapterVersion ||
    result.declarationFingerprint !== frozen.declarationFingerprint ||
    result.configurationRevision !== frozen.configurationRevision
  ) fail();
  return result;
}

/** Validates objects before reading their members. */
function record(input: JsonValue | undefined): { [key: string]: JsonValue } {
  if (!input || typeof input !== "object" || Array.isArray(input)) fail();
  return input;
}

/** Accepts required text without including it in a failure. */
function string(input: JsonValue | undefined): string {
  if (typeof input !== "string" || input.length === 0) fail();
  return input;
}

/** The TypeScript contract permits only exactly representable counters and limits. */
function integer(input: JsonValue | undefined, minimum: number): number {
  if (
    typeof input !== "number" || !Number.isSafeInteger(input) || input < minimum
  ) fail();
  return input;
}

/** Sensitive input is deliberately absent from this bounded error. */
function fail(): never {
  throw new PluginMethodError(
    INVALID_PARAMS,
    "invalid or unapplied Agent model context",
  );
}
