/**
 * Cove catalogue proxy: a small, fixed API in front of IGDB. Clients send
 * parameters only; Apicalypse queries are built here. No pass-through.
 */
import type {
  Game,
  MatchCandidate,
  ProxyError,
  ProxyErrorCode,
  ProxyGame,
  ProxyMatchResponse,
  ProxyPage,
  ProxyPricesResponse,
  ProxyTimeToBeatResponse,
  TimeToBeat,
} from "@cove/shared";
import { Hono } from "hono";
import type { z } from "zod";
import { cached, cacheApi, TTL, type JsonCache } from "./cache.ts";
import type { Env } from "./env.ts";
import { IgdbClient, UpstreamError } from "./igdb-client.ts";
import {
  buildBrowseBody,
  buildDetailsBody,
  buildGamesByIdsBody,
  buildSearchBody,
  buildSimilarIdsBody,
  buildTimeToBeatBody,
  buildWhere,
  PAGE_SIZE,
} from "./igdb-query.ts";
import { mapGame, type IgdbGame } from "./mapping.ts";
import { normalize, score } from "./match.ts";
import { ItadClient } from "./itad.ts";
import { browseQuery, gameId, matchBody, pricesBody, searchQuery, steamAppId, steamId, timeToBeatBody } from "./schemas.ts";
import { SteamClient, steamAchievements, steamOwned, steamProfile, steamRarity, steamSchema } from "./steam.ts";

const SEARCH_LIMIT = 20;

/** A row of IGDB's `game_time_to_beats` (seconds). */
interface IgdbTimeToBeat {
  game_id: number;
  hastily?: number | null;
  normally?: number | null;
  completely?: number | null;
  count?: number | null;
}

/** Seconds, or null for missing and nonsense values. */
function seconds(value: number | null | undefined): number | null {
  return typeof value === "number" && Number.isFinite(value) && value > 0 ? Math.round(value) : null;
}

/** SHA-256 in hex: a short cache key for a long id list. */
async function digest(text: string): Promise<string> {
  const hash = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text));
  return Array.from(new Uint8Array(hash), (b) => b.toString(16).padStart(2, "0")).join("");
}

function timeToBeat(row: IgdbTimeToBeat): TimeToBeat | null {
  const value = { hastily: seconds(row.hastily), normally: seconds(row.normally), completely: seconds(row.completely) };
  if (value.hastily === null && value.normally === null && value.completely === null) return null;
  return { ...value, count: typeof row.count === "number" ? Math.max(0, Math.round(row.count)) : 0 };
}
const MATCH_MIN_SCORE = 0.55;

export interface Deps {
  fetch?: typeof fetch;
  cache?: JsonCache;
  now?: () => number;
}

class ApiError extends Error {
  constructor(
    readonly status: 400 | 404 | 429 | 500 | 502 | 503,
    readonly code: ProxyErrorCode,
    message: string,
  ) {
    super(message);
  }
}

function errorBody(code: ProxyErrorCode, message: string): ProxyError {
  return { error: { code, message } };
}

function parse<T extends z.ZodType>(schema: T, input: unknown): z.output<T> {
  const result = schema.safeParse(input);
  if (!result.success) {
    const message = result.error.issues.map((i) => `${i.path.join(".") || "input"}: ${i.message}`).join("; ");
    throw new ApiError(400, "invalid_request", message);
  }
  return result.data;
}

export function createApp(deps: Deps = {}) {
  const app = new Hono<{ Bindings: Env; Variables: { igdb: IgdbClient; steam: SteamClient; cache: JsonCache } }>();
  const now = deps.now ?? Date.now;

  // Per-client-IP limit (60 req/min) against abuse.
  app.use("/v1/*", async (c, next) => {
    const ip = c.req.header("CF-Connecting-IP") ?? "unknown";
    const { success } = await c.env.IP_LIMIT.limit({ key: ip });
    if (!success) throw new ApiError(429, "rate_limited", "Too many requests, slow down.");
    c.set("igdb", new IgdbClient(c.env, deps.fetch, now));
    c.set("steam", new SteamClient(c.env, deps.fetch));
    c.set("cache", deps.cache ?? cacheApi(caches.default));
    await next();
  });

  app.get("/v1/health", (c) => c.json({ ok: true }));

  app.get("/v1/search", async (c) => {
    const { q } = parse(searchQuery, c.req.query());
    const body = await cached(c.get("cache"), `search:${q.toLowerCase()}`, TTL.search, async () => {
      const raw = await c.get("igdb").query<IgdbGame[]>("games", buildSearchBody(q, SEARCH_LIMIT));
      return { items: raw.map(mapGame), page: 0, hasMore: false, total: null } satisfies ProxyPage;
    });
    return c.json(body);
  });

  app.get("/v1/browse", async (c) => {
    const params = parse(browseQuery, c.req.query());
    const key = `browse:${JSON.stringify(params)}`;
    const body = await cached(c.get("cache"), key, TTL.search, async () => {
      const nowSeconds = Math.floor(now() / 1000);
      const igdb = c.get("igdb");
      const raw = await igdb.query<IgdbGame[]>("games", buildBrowseBody(params, nowSeconds));
      const items = raw.map(mapGame);
      const offset = params.page * PAGE_SIZE;
      if (params.q) {
        // IGDB's count endpoint ignores `search`; estimate from the page.
        return { items, page: params.page, hasMore: items.length === PAGE_SIZE, total: null } satisfies ProxyPage;
      }
      const { count } = await igdb.query<{ count: number }>("games/count", `where ${buildWhere(params, nowSeconds)};`);
      return { items, page: params.page, hasMore: offset + items.length < count, total: count } satisfies ProxyPage;
    });
    return c.json(body);
  });

  async function details(c: { get: (k: "igdb") => IgdbClient }, id: number): Promise<Game> {
    const raw = await c.get("igdb").query<IgdbGame[]>("games", buildDetailsBody(id));
    const first = raw[0];
    if (!first) throw new ApiError(404, "not_found", `Game ${String(id)} not found`);
    return mapGame(first);
  }

  app.get("/v1/game/:id", async (c) => {
    const id = parse(gameId, c.req.param("id"));
    const body = await cached(c.get("cache"), `game:${String(id)}`, TTL.details, async () => {
      return { game: await details(c, id) } satisfies ProxyGame;
    });
    return c.json(body);
  });

  app.get("/v1/similar/:id", async (c) => {
    const id = parse(gameId, c.req.param("id"));
    const body = await cached(c.get("cache"), `similar:${String(id)}`, TTL.details, async () => {
      const igdb = c.get("igdb");
      const [entry] = await igdb.query<{ similar_games?: number[] }[]>("games", buildSimilarIdsBody(id));
      if (!entry) throw new ApiError(404, "not_found", `Game ${String(id)} not found`);
      const ids = (entry.similar_games ?? []).slice(0, 10);
      const items = ids.length > 0 ? (await igdb.query<IgdbGame[]>("games", buildGamesByIdsBody(ids))).map(mapGame) : [];
      return { items, page: 0, hasMore: false, total: items.length } satisfies ProxyPage;
    });
    return c.json(body);
  });

  app.post("/v1/match", async (c) => {
    const json: unknown = await c.req.json().catch(() => {
      throw new ApiError(400, "invalid_request", "body must be JSON");
    });
    const { title } = parse(matchBody, json);
    const body = await cached(c.get("cache"), `match:${normalize(title)}`, TTL.search, async () => {
      const raw = await c.get("igdb").query<IgdbGame[]>("games", buildSearchBody(title, 10));
      const candidates: MatchCandidate[] = raw
        .map(mapGame)
        .map((game) => ({ game, score: Math.round(score(title, game.title) * 1000) / 1000 }))
        .filter((m) => m.score >= MATCH_MIN_SCORE)
        .sort((a, b) => b.score - a.score)
        .slice(0, 5);
      return { candidates } satisfies ProxyMatchResponse;
    });
    return c.json(body);
  });

  // One cache entry per set of ids (cache calls count as subrequests, so not
  // one per game); the core caches each game itself and only asks for new ones.
  app.post("/v1/time-to-beat", async (c) => {
    const json: unknown = await c.req.json().catch(() => {
      throw new ApiError(400, "invalid_request", "body must be JSON");
    });
    const ids = [...new Set(parse(timeToBeatBody, json).ids)].sort((a, b) => a - b);
    const body = await cached(c.get("cache"), `ttb:${await digest(ids.join(","))}`, TTL.timeToBeat, async () => {
      const rows = await c.get("igdb").query<IgdbTimeToBeat[]>("game_time_to_beats", buildTimeToBeatBody(ids));
      const items: Record<string, TimeToBeat> = {};
      for (const row of rows) {
        const value = timeToBeat(row);
        if (value && ids.includes(row.game_id)) items[String(row.game_id)] = value;
      }
      return { items } satisfies ProxyTimeToBeatResponse;
    });
    return c.json(body);
  });

  // ───────────── prices (IsThereAnyDeal; key stays in the Worker) ─────────────

  // One cache entry per country and title set, like times to beat.
  app.post("/v1/prices", async (c) => {
    const json: unknown = await c.req.json().catch(() => {
      throw new ApiError(400, "invalid_request", "body must be JSON");
    });
    const { country, games } = parse(pricesBody, json);
    const titles = [...new Set(games.map((g) => g.title))].sort();
    const body = await cached(c.get("cache"), `prices:${country}:${await digest(titles.join("\n"))}`, TTL.prices, async () => {
      const itad = new ItadClient(c.env, deps.fetch);
      const ids = await itad.ids(titles);
      const prices = await itad.prices([...new Set(ids.values())], country, new Date(now()));
      const byTitle: Record<string, ReturnType<typeof prices.get> | null> = {};
      for (const title of titles) {
        const id = ids.get(title);
        byTitle[title] = (id ? prices.get(id) : undefined) ?? null;
      }
      return byTitle;
    });
    const items: ProxyPricesResponse["items"] = {};
    for (const game of games) items[game.key] = body[game.title] ?? null;
    return c.json({ items } satisfies ProxyPricesResponse);
  });

  // ───────────── Steam (key stays in the Worker) ─────────────

  app.get("/v1/steam/profile/:steamid", async (c) => {
    const id = parse(steamId, c.req.param("steamid"));
    const body = await cached(c.get("cache"), `steam:profile:${id}`, TTL.steamUser, async () => {
      const profile = await steamProfile(c.get("steam"), id);
      if (!profile) throw new ApiError(404, "not_found", "Steam profile not found");
      return profile;
    });
    return c.json(body);
  });

  app.get("/v1/steam/owned/:steamid", async (c) => {
    const id = parse(steamId, c.req.param("steamid"));
    const body = await cached(c.get("cache"), `steam:owned:${id}`, TTL.steamUser, () => steamOwned(c.get("steam"), id));
    return c.json(body);
  });

  app.get("/v1/steam/achievements/:steamid/:appid", async (c) => {
    const id = parse(steamId, c.req.param("steamid"));
    const appId = parse(steamAppId, c.req.param("appid"));
    const steam = c.get("steam");
    const cache = c.get("cache");
    const body = await cached(cache, `steam:achievements:${id}:${appId}`, TTL.steamUser, async () => {
      const schema = await cached(cache, `steam:schema:${appId}`, TTL.steamSchema, () => steamSchema(steam, appId));
      const rarity = await cached(cache, `steam:rarity:${appId}`, TTL.steamSchema, () => steamRarity(steam, appId));
      return steamAchievements(steam, id, appId, schema, rarity);
    });
    return c.json(body);
  });

  app.notFound((c) => c.json(errorBody("not_found", "Unknown endpoint"), 404));

  app.onError((error, c) => {
    if (error instanceof ApiError) return c.json(errorBody(error.code, error.message), error.status);
    if (error instanceof UpstreamError) {
      const code: ProxyErrorCode =
        error.status === 500 ? "not_configured" : error.status === 503 ? "upstream_unavailable" : "upstream_error";
      return c.json(errorBody(code, error.message), error.status === 500 ? 500 : error.status === 503 ? 503 : 502);
    }
    console.error(error);
    return c.json(errorBody("upstream_unavailable", "Catalogue temporarily unavailable"), 503);
  });

  return app;
}
