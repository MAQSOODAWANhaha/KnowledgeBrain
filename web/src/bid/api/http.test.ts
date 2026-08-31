import { NetworkTransportError } from "../../api";
import { describe, expect, it } from "../authoring/harness";
import { v2Request } from "./http";

Object.defineProperty(globalThis, "localStorage", {
  configurable: true,
  value: {
    getItem: () => null,
    setItem: () => undefined,
    removeItem: () => undefined,
  },
});

function response(overrides: Partial<Response>): Response {
  return {
    ok: true,
    status: 202,
    statusText: "Accepted",
    headers: new Headers(),
    text: async () => "{}",
    ...overrides,
  } as Response;
}

describe("V2 HTTP dispatch uncertainty", () => {
  it("classifies response stream rejection as transport uncertainty", async () => {
    const previous = globalThis.fetch;
    globalThis.fetch = async () => response({
      text: async () => { throw new TypeError("truncated body"); },
    });
    let error: unknown;
    try {
      await v2Request("/api/v2/test", { method: "POST", body: "{}" });
    } catch (value) {
      error = value;
    } finally {
      globalThis.fetch = previous;
    }
    expect(error instanceof NetworkTransportError).toBe(true);
  });

  it("classifies malformed committed mutation JSON as transport uncertainty", async () => {
    const previous = globalThis.fetch;
    globalThis.fetch = async () => response({ text: async () => "{truncated" });
    let error: unknown;
    try {
      await v2Request("/api/v2/test", { method: "POST", body: "{}" });
    } catch (value) {
      error = value;
    } finally {
      globalThis.fetch = previous;
    }
    expect(error instanceof NetworkTransportError).toBe(true);
  });
});
