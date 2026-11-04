/**
 * Cove catalogue proxy: a small, fixed API in front of IGDB. Clients send
 * parameters only; Apicalypse queries are built here. No pass-through.
 */
import type { Game, MatchCandidate, ProxyError, ProxyErrorCode, ProxyGame, ProxyMatchResponse, ProxyPage } from "@cove/shared";
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
  buildWhere,
  PAGE_SIZE,
} from "./igdb-query.ts";
import { mapGame, type IgdbGame } from "./mapping.ts";
import { normalize, score } from "./match.ts";
import { browseQuery, gameId, matchBody, searchQuery, steamAppId, steamId } from "./schemas.ts";
import { SteamClient, steamAchievements, steamOwned, steamProfile, steamRarity, steamSchema } from "./steam.ts";

const SEARCH_LIMIT = 20;
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
