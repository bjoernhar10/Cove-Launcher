import schemaJson from "@cove/shared/schemas/models.schema.json" with { type: "json" };
import type { AnySchema } from "ajv";
import { describe, expect, it } from "vitest";
import { createApp } from "../src/app.ts";
import { IgdbGate, MAX_WAIT_MS } from "../src/gate.ts";
import { buildBrowseBody } from "../src/igdb-query.ts";
import { FakeCache, fakeEnv, fakeNetwork, WITCHER, type IgdbCall } from "./fakes.ts";

const schema = schemaJson as AnySchema;
const { default: Ajv } = await import("ajv");
const ajv = new Ajv({ allErrors: true, strict: false });
ajv.addSchema(schema);

function expectShape(definition: string, value: unknown): void {
  const validate = ajv.getSchema(`https://cove.local/schemas/models.schema.json#/definitions/${definition}`);
  if (!validate) throw new Error(definition);
  expect(validate(value), JSON.stringify(validate.errors)).toBe(true);
}

function setup(igdb: (call: IgdbCall) => unknown = () => [WITCHER], options: Parameters<typeof fakeEnv>[0] = {}) {
  const net = fakeNetwork(igdb);
  const cache = new FakeCache();
  const bindings = fakeEnv(options);
  const app = createApp({ fetch: net.fetchFn, cache });
  const request = (path: string, init: { method?: string; body?: string } = {}) =>
    app.request(path, { ...init, headers: { "CF-Connecting-IP": "203.0.113.7" } }, bindings.env);
  return { ...bindings, net, cache, request };
}

describe("validation", () => {
  it.each([
    ["/v1/search?q=a", "q"],
    ["/v1/browse?genre=jazz", "genre"],
    ["/v1/browse?year=1800", "year"],
    ["/v1/browse?year=2022-2020", "year"],
    ["/v1/browse?page=-1", "page"],
    ["/v1/browse?sort=random", "sort"],
    ["/v1/game/abc", "input"],
    ["/v1/similar/0", "input"],
  ])("rejects %s", async (path, field) => {
    const { request, net } = setup();
    const res = await request(path);
    expect(res.status).toBe(400);
    const body = await res.json<{ error: { code: string; message: string } }>();
    expectShape("ProxyError", body);
    expect(body.error.code).toBe("invalid_request");
    expect(body.error.message).toContain(field);
    expect(net.calls).toHaveLength(0);
  });

  it("rejects match requests without a title and unknown endpoints", async () => {
    const { request } = setup();
    expect((await request("/v1/match", { method: "POST", body: "{}" })).status).toBe(400);
    expect((await request("/v1/match", { method: "POST", body: "nope" })).status).toBe(400);
    const res = await request("/v4/games", { method: "POST", body: "fields *;" });
    expect(res.status).toBe(404);
    expect(await res.json()).toEqual({ error: { code: "not_found", message: "Unknown endpoint" } });
  });
});

describe("endpoints", () => {
  it("browse builds the Apicalypse query itself and returns ProxyPage", async () => {
    const { request, net } = setup((call) => (call.endpoint === "games/count" ? { count: 25 } : [WITCHER]));
    const res = await request("/v1/browse?genre=rpg,horror&platform=linux&year=2018-2021&rating=80&sort=popular&page=1");
    expect(res.status).toBe(200);
    const body = await res.json<unknown>();
    expectShape("ProxyPage", body);
    // offset 24 + 1 item = 25 = total → last page
    expect(body).toMatchObject({ page: 1, total: 25, hasMore: false });

    const games = net.calls.find((c) => c.endpoint === "games");
    expect(games?.body).toContain("platforms = (3)");
    expect(games?.body).toContain("(genres = (12) | themes = (19))");
    expect(games?.body).toContain("first_release_date >= 1514764800 & first_release_date < 1640995200");
    expect(games?.body).toContain("total_rating >= 80");
    expect(games?.body).toContain("sort total_rating_count desc;");
    expect(games?.body).toMatch(/limit 24; offset 24;$/);
    expect(games?.headers.get("Client-ID")).toBe("cid");
    expect(games?.headers.get("Authorization")).toBe("Bearer tok1");
  });

  it("maps games to the shared Game type with direct IGDB image URLs", async () => {
    const { request } = setup();
    const { game } = await (await request("/v1/game/1942")).json<{ game: Record<string, unknown> }>();
    expectShape("ProxyGame", { game });
    expect(game).toMatchObject({
      id: "igdb-1942",
      genres: ["rpg", "adventure", "action"],
      platforms: ["windows", "linux"],
      releaseDate: "2015-05-19",
      developer: "CD Projekt RED",
      rating: 93,
      summary: "An open-world RPG set in a dark fantasy world of monsters.",
    });
    expect(JSON.stringify(game)).toContain("https://images.igdb.com/igdb/image/upload/t_cover_big/co1wyy.jpg");
    expect(game.features).toEqual({
      themes: ["Action"],
      gameModes: ["Single player"],
      perspectives: ["Third person"],
      keywords: ["open world"],
    });
  });

  it("search uses `search` without sort, similar resolves ids", async () => {
    const { request, net } = setup((call) => {
      if (call.body.startsWith("fields similar_games")) return [{ id: 1942, similar_games: [5, 6] }];
      if (call.body.includes("where id = (5,6)")) return [{ id: 5, name: "Five" }, { id: 6, name: "Six" }];
      return [WITCHER];
    });
    const search = await (await request("/v1/search?q=Witch%22er")).json<unknown>();
    expectShape("ProxyPage", search);
    expect(net.calls[0]?.body).toMatch(/^search "witch\\"er"; /i);
    expect(net.calls[0]?.body).not.toContain("sort");

    const similar = await (await request("/v1/similar/1942")).json<{ items: { id: string }[] }>();
    expect(similar.items.map((g) => g.id)).toEqual(["igdb-5", "igdb-6"]);
  });

  it("game details answer 404 for unknown ids", async () => {
    const { request } = setup(() => []);
    const res = await request("/v1/game/77");
    expect(res.status).toBe(404);
    expect((await res.json<{ error: { code: string } }>()).error.code).toBe("not_found");
  });

  it("match returns scored candidates, best first", async () => {
    const { request } = setup(() => [
      { id: 2, name: "The Witcher 2" },
      WITCHER,
      { id: 3, name: "Stardew Valley" },
    ]);
    const res = await request("/v1/match", { method: "POST", body: JSON.stringify({ title: "the_witcher_3 wild hunt" }) });
    const body = await res.json<{ candidates: { game: { id: string }; score: number }[] }>();
    expectShape("ProxyMatchResponse", body);
    expect(body.candidates[0]?.game.id).toBe("igdb-1942");
    expect(body.candidates[0]?.score).toBe(1);
    expect(body.candidates.some((c) => c.game.id === "igdb-3")).toBe(false);
  });
});

describe("caching", () => {
  it("caches responses with the right TTL (search 24 h, details 7 days)", async () => {
    const { request, net, cache } = setup();
    await request("/v1/search?q=witcher");
    await request("/v1/search?q=WITCHER");
    await request("/v1/game/1942");
    await request("/v1/game/1942");
    expect(net.calls).toHaveLength(2);
    expect(cache.ttls.get("search:witcher")).toBe(86_400);
    expect(cache.ttls.get("game:1942")).toBe(604_800);
  });

  it("stores the token in KV and reuses it until shortly before expiry", async () => {
    const { request, net, kv, env } = setup();
    await request("/v1/search?q=one");
    await request("/v1/search?q=two");
    expect(net.tokenRequests).toBe(1);
    expect(kv.puts[0]?.ttl).toBe(5_000_000 - 600);

    // Token about to expire → renewed.
    await env.TOKENS.put("igdb:token", JSON.stringify({ accessToken: "old", expiresAt: Math.floor(Date.now() / 1000) + 60 }));
    await request("/v1/search?q=three");
    expect(net.tokenRequests).toBe(2);
    expect(net.calls.at(-1)?.headers.get("Authorization")).toBe("Bearer tok2");
  });

  it("refreshes a rejected token once", async () => {
    let first = true;
    const { request, net } = setup(() => {
      if (first) {
        first = false;
        return new Response("unauthorized", { status: 401 });
      }
      return [WITCHER];
    });
    expect((await request("/v1/search?q=retry")).status).toBe(200);
    expect(net.tokenRequests).toBe(2);
  });
});

describe("rate limiting", () => {
  it("retries IGDB 429 and gives up with 503", async () => {
    let n = 0;
    const flaky = setup(() => (++n === 1 ? new Response("slow down", { status: 429 }) : [WITCHER]));
    expect((await flaky.request("/v1/search?q=flaky")).status).toBe(200);

    const busy = setup(() => new Response("slow down", { status: 429 }));
    const res = await busy.request("/v1/search?q=busy");
    expect(res.status).toBe(503);
    expect((await res.json<{ error: { code: string } }>()).error.code).toBe("upstream_unavailable");
  }, 10_000);

  it("every IGDB request waits for a slot of the global gate", async () => {
    const { request, gate } = setup((call) => (call.endpoint === "games/count" ? { count: 1 } : [WITCHER]));
    await request("/v1/browse");
    expect(gate.calls).toBe(2); // games + count
  });

  it("limits requests per client IP", async () => {
    const { request } = setup(undefined, { ipLimit: 2 });
    expect((await request("/v1/health")).status).toBe(200);
    expect((await request("/v1/health")).status).toBe(200);
    const res = await request("/v1/health");
    expect(res.status).toBe(429);
    expect((await res.json<{ error: { code: string } }>()).error.code).toBe("rate_limited");
  });

  it("gate hands out 250 ms slots and rejects an overlong queue", async () => {
    let now = 1_000;
    const gate = new IgdbGate({} as DurableObjectState, {}, () => now);
    const wait = async () => (await gate.fetch().json<{ waitMs: number }>()).waitMs;
    expect(await wait()).toBe(0);
    expect(await wait()).toBe(250);
    expect(await wait()).toBe(500);
    now += 1_000;
    expect(await wait()).toBe(0);
    for (let i = 0; i < MAX_WAIT_MS / 250; i += 1) await wait();
    expect(gate.fetch().status).toBe(503);
  });
});

it("reports missing Worker secrets clearly", async () => {
  const { request } = setup(undefined, { secrets: false });
  const res = await request("/v1/search?q=witcher");
  expect(res.status).toBe(500);
  expect((await res.json<{ error: { code: string } }>()).error.code).toBe("not_configured");
});

it("query builder keeps the default desktop platform filter", () => {
  const body = buildBrowseBody(
    { q: "", genres: [], platforms: [], years: null, minRating: null, sort: "newest", page: 0 },
    1_800_000_000,
  );
  expect(body).toContain("platforms = (6,3,14)");
  expect(body).toContain("first_release_date <= 1800000000");
});

it("calls fetch without a foreign `this` (Workers reject that with 'Illegal invocation')", async () => {
  const net = fakeNetwork(() => [WITCHER]);
  // Mimic workerd: fetch must not be invoked as a method of another object.
  const strictFetch = function (this: unknown, input: RequestInfo | URL, init?: RequestInit) {
    if (this !== undefined && this !== globalThis) throw new TypeError("Illegal invocation");
    return net.fetchFn(input, init);
  } as typeof fetch;
  const bindings = fakeEnv();
  const app = createApp({ fetch: strictFetch, cache: new FakeCache() });
  const res = await app.request("/v1/search?q=witcher", { headers: { "CF-Connecting-IP": "1.2.3.4" } }, bindings.env);
  expect(res.status).toBe(200);
});
