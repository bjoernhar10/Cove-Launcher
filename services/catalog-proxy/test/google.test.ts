import schemaJson from "@cove/shared/schemas/models.schema.json" with { type: "json" };
import { describe, expect, it } from "vitest";
import { createApp } from "../src/app.ts";
import { FakeCache, fakeEnv } from "./fakes.ts";

const { default: Ajv } = await import("ajv");
const ajv = new Ajv({ allErrors: true, strict: false });
ajv.addSchema(schemaJson);

const VERIFIER = "a".repeat(43);

function setup(answer: (form: URLSearchParams) => Response, secrets = true) {
  const calls: URLSearchParams[] = [];
  const fetchFn: typeof fetch = (input, init) => {
    const url = new URL(input instanceof Request ? input.url : String(input));
    expect(url.href).toBe("https://oauth2.googleapis.com/token");
    const form = new URLSearchParams(typeof init?.body === "string" ? init.body : "");
    calls.push(form);
    return Promise.resolve(answer(form));
  };
  const cache = new FakeCache();
  const app = createApp({ fetch: fetchFn, cache });
  const { env } = fakeEnv({ secrets });
  const request = (body: unknown) =>
    app.request("/v1/oauth/google/token", { method: "POST", body: JSON.stringify(body), headers: { "CF-Connecting-IP": "203.0.113.7" } }, env);
  return { calls, cache, request };
}

describe("google token", () => {
  it("adds the client secret to a code exchange and caches nothing", async () => {
    const { calls, cache, request } = setup(() => Response.json({ access_token: "at", refresh_token: "rt", expires_in: 3599 }));
    const res = await request({ grantType: "authorization_code", code: "c0de", codeVerifier: VERIFIER, redirectUri: "http://127.0.0.1:41234/callback" });
    expect(res.status).toBe(200);
    expect(res.headers.get("Cache-Control")).toBe("no-store");
    const body: unknown = await res.json();
    expect(body).toEqual({ accessToken: "at", refreshToken: "rt", expiresIn: 3599 });
    const validate = ajv.getSchema("https://cove.local/schemas/models.schema.json#/definitions/ProxyOAuthToken");
    expect(validate?.(body), JSON.stringify(validate?.errors)).toBe(true);
    const form = calls[0];
    expect(form?.get("client_id")).toBe("gid.apps.googleusercontent.com");
    expect(form?.get("client_secret")).toBe("gsecret");
    expect(form?.get("grant_type")).toBe("authorization_code");
    expect(form?.get("code_verifier")).toBe(VERIFIER);
    expect(cache.data.size).toBe(0);
  });

  it("refreshes and reports a refused token as invalid_grant", async () => {
    const ok = setup(() => Response.json({ access_token: "at2", expires_in: 3600 }));
    const res = await ok.request({ grantType: "refresh_token", refreshToken: "rt" });
    expect(await res.json()).toEqual({ accessToken: "at2", refreshToken: null, expiresIn: 3600 });
    expect(ok.calls[0]?.get("refresh_token")).toBe("rt");

    const denied = setup(() => Response.json({ error: "invalid_grant" }, { status: 400 }));
    const bad = await denied.request({ grantType: "refresh_token", refreshToken: "old" });
    expect(bad.status).toBe(400);
    expect(await bad.json()).toMatchObject({ error: { code: "invalid_grant" } });
  });

  it("only takes Cove's loopback address and is not configured without the secret", async () => {
    const { calls, request } = setup(() => Response.json({}));
    const res = await request({ grantType: "authorization_code", code: "c", codeVerifier: VERIFIER, redirectUri: "https://evil.example/callback" });
    expect(res.status).toBe(400);
    expect(calls).toHaveLength(0);

    const bare = setup(() => Response.json({}), false);
    const res2 = await bare.request({ grantType: "refresh_token", refreshToken: "rt" });
    expect(res2.status).toBe(500);
    expect(await res2.json()).toMatchObject({ error: { code: "not_configured" } });
  });
});
