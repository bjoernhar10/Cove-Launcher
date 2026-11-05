import { readFileSync } from "node:fs";
import { Ajv2020 } from "ajv/dist/2020.js";
import { describe, expect, it } from "vitest";
import { JSONRPC_VERSION, type JsonRpcRequest, type JsonRpcSuccess } from "./jsonrpc.ts";
import type { PingParams, PingResult, SetLanguageParams, SetLanguageResult } from "./methods.ts";

function loadSchema(name: string): object {
  const url = new URL(`../schemas/${name}`, import.meta.url);
  return JSON.parse(readFileSync(url, "utf8")) as object;
}

const ajv = new Ajv2020({ strict: true, allowUnionTypes: true });
ajv.addSchema(loadSchema("jsonrpc.schema.json"));
ajv.addSchema(loadSchema("system.ping.schema.json"));
ajv.addSchema(loadSchema("system.setLanguage.schema.json"));

describe("JSON schemas", () => {
  it("accepts a valid system.ping request", () => {
    const request: JsonRpcRequest<"system.ping", PingParams> = {
      jsonrpc: JSONRPC_VERSION,
      id: 1,
      method: "system.ping",
      params: { message: "hi" },
    };
    expect(ajv.validate("https://cove.local/schemas/jsonrpc.schema.json", request)).toBe(true);
    expect(
      ajv.validate("https://cove.local/schemas/system.ping.schema.json#/$defs/params", request.params),
    ).toBe(true);
  });

  it("accepts a valid system.ping result", () => {
    const response: JsonRpcSuccess<PingResult> = {
      jsonrpc: JSONRPC_VERSION,
      id: 1,
      result: { service: "core", version: "0.1.0", message: "pong" },
    };
    expect(ajv.validate("https://cove.local/schemas/jsonrpc.schema.json", response)).toBe(true);
    expect(
      ajv.validate("https://cove.local/schemas/system.ping.schema.json#/$defs/result", response.result),
    ).toBe(true);
  });

  it("accepts system.setLanguage and rejects unknown languages", () => {
    const id = "https://cove.local/schemas/system.setLanguage.schema.json";
    const params: SetLanguageParams = { language: "en" };
    const result: SetLanguageResult = { language: "en" };
    expect(ajv.validate(`${id}#/$defs/params`, params)).toBe(true);
    expect(ajv.validate(`${id}#/$defs/result`, result)).toBe(true);
    expect(ajv.validate(`${id}#/$defs/params`, { language: "fr" })).toBe(false);
  });

  it("rejects a message with the wrong protocol version", () => {
    const bad = { jsonrpc: "1.0", id: 1, method: "system.ping" };
    expect(ajv.validate("https://cove.local/schemas/jsonrpc.schema.json", bad)).toBe(false);
  });
});
