import schemaJson from "@cove/shared/schemas/models.schema.json" with { type: "json" };
import { describe, expect, it } from "vitest";
import { createApp } from "../src/app.ts";
import { FakeCache, fakeEnv, fakeNetwork, WITCHER } from "./fakes.ts";

const { default: Ajv } = await import("ajv");
const ajv = new Ajv({ allErrors: true, strict: false });
ajv.addSchema(schemaJson);

const ORBIT = "018d937f-0000-7000-8000-000000000001";
const TIDE = "018d937f-0000-7000-8000-000000000002";

function itad(url: URL, body: string, headers: Headers): unknown {
  expect(headers.get("ITAD-API-Key")).toBe("itadkey");
  expect(url.searchParams.has("key")).toBe(false);
  switch (url.pathname) {
    case "/lookup/id/title/v1": {
      const titles = JSON.parse(body) as string[];
      return Object.fromEntries(titles.map((t) => [t, t === "Paper Orbit" ? ORBIT : null]));
    }
    case "/games/lookup/v1":
      return url.searchParams.get("title") === "Tidewater" ? { found: true, game: { id: TIDE, slug: "tidewater", title: "Tidewater", type: "game" } } : { found: false };
    case "/games/prices/v3":
      expect(url.searchParams.get("country")).toBe("DE");
      return [
        {
          id: ORBIT,
          historyLow: { all: { amount: 4.99, amountInt: 499, currency: "EUR" }, y1: null, m3: null },
          deals: [
            { shop: { id: 61, name: "Steam" }, price: { amount: 9.99, amountInt: 999, currency: "EUR" }, regular: { amount: 19.99, amountInt: 1999, currency: "EUR" }, cut: 50, voucher: null, storeLow: null, flag: null, drm: [], platforms: [], timestamp: "2026-11-10T10:00:00+01:00", expiry: "2026-11-20T18:00:00+01:00", url: "https://itad.link/steam-orbit" },
            { shop: { id: 35, name: "GOG" }, price: { amount: 7.99, amountInt: 799, currency: "EUR" }, regular: { amount: 19.99, amountInt: 1999, currency: "EUR" }, cut: 60, voucher: null, storeLow: null, flag: "H", drm: [], platforms: [], timestamp: "2026-11-10T10:00:00+01:00", expiry: null, url: "https://itad.link/gog-orbit" },
          ],
        },
        { id: TIDE, historyLow: { all: null, y1: null, m3: null }, deals: [] },
      ];
    case "/games/overview/v2":
      return { prices: [{ id: ORBIT, current: null, lowest: null, bundled: 0, urls: { game: "https://isthereanydeal.com/game/paper-orbit/info/" } }], bundles: [] };
    default:
      return new Response("unknown", { status: 404 });
  }
}

function setup(secrets = true) {
  const net = fakeNetwork(() => [WITCHER], undefined, itad);
  const cache = new FakeCache();
  const bindings = fakeEnv({ secrets });
  const app = createApp({ fetch: net.fetchFn, cache, now: () => Date.parse("2026-11-13T12:00:00Z") });
  const request = (body: unknown) =>
    app.request("/v1/prices", { method: "POST", body: JSON.stringify(body), headers: { "CF-Connecting-IP": "203.0.113.7" } }, bindings.env);
  return { net, cache, request };
}

describe("prices", () => {
  it("finds games by title and returns the cheapest deals first", async () => {
    const { request, net } = setup();
    const res = await request({ country: "DE", games: [{ key: "igdb-1", title: "Paper Orbit" }, { key: "igdb-2", title: "Tidewater" }, { key: "igdb-3", title: "Nowhere" }] });
    expect(res.status).toBe(200);
    const body = await res.json<{ items: Record<string, { best: { shop: string; cut: number; url: string } | null; deals: unknown[]; historyLow: { amount: number } | null; url: string | null } | null> }>();
    const validate = ajv.getSchema("https://cove.local/schemas/models.schema.json#/definitions/ProxyPricesResponse");
    expect(validate?.(body), JSON.stringify(validate?.errors)).toBe(true);
    const orbit = body.items["igdb-1"];
    expect(orbit?.best).toMatchObject({ shop: "GOG", cut: 60, url: "https://itad.link/gog-orbit" });
    expect(orbit?.deals).toHaveLength(2);
    expect(orbit?.historyLow?.amount).toBe(4.99);
    expect(orbit?.url).toBe("https://isthereanydeal.com/game/paper-orbit/info/");
    expect(body.items["igdb-2"]).toMatchObject({ best: null, deals: [], historyLow: null });
    expect(body.items["igdb-3"]).toBeNull();
    // The same titles again come from the cache.
    const calls = net.itadCalls.length;
    await request({ country: "DE", games: [{ key: "x", title: "Nowhere" }, { key: "y", title: "Tidewater" }, { key: "z", title: "Paper Orbit" }] });
    expect(net.itadCalls.length).toBe(calls);
  });

  it("checks the input and needs the key", async () => {
    const { request } = setup();
    expect((await request({ country: "de", games: [{ key: "a", title: "b" }] })).status).toBe(400);
    expect((await request({ country: "DE", games: [] })).status).toBe(400);
    const many = Array.from({ length: 51 }, (_, i) => ({ key: String(i), title: `t${String(i)}` }));
    expect((await request({ country: "DE", games: many })).status).toBe(400);
    const res = await setup(false).request({ country: "DE", games: [{ key: "a", title: "b" }] });
    expect(res.status).toBe(500);
    expect(await res.json()).toMatchObject({ error: { code: "not_configured" } });
  });
});
