/**
 * Response cache on the Cache API (per data centre, no write limits).
 * KV is only used for the token: its free tier allows ~1000 writes/day,
 * far too few for caching every catalogue response.
 */
export const TTL = {
  /** Search, browse, match */
  search: 24 * 60 * 60,
  /** Game details, similar games */
  details: 7 * 24 * 60 * 60,
  /** Steam user data (profile, owned games, progress) */
  steamUser: 60 * 60,
  /** Steam achievement schema and global rarity */
  steamSchema: 7 * 24 * 60 * 60,
  /** Times to beat (change slowly) */
  timeToBeat: 7 * 24 * 60 * 60,
} as const;

/** Bump when the response format changes, so old cached answers are ignored. */
export const CACHE_VERSION = 2;

function cacheKey(key: string): Request {
  return new Request(`https://cache.cove.internal/v${String(CACHE_VERSION)}/${encodeURIComponent(key)}`);
}

export interface JsonCache {
  get(key: string): Promise<unknown>;
  put(key: string, value: unknown, ttlSeconds: number): Promise<void>;
}

/** Cache API backed implementation (`caches.default`). */
export function cacheApi(cache: Cache): JsonCache {
  return {
    async get(key) {
      const hit = await cache.match(cacheKey(key));
      return hit ? await hit.json<unknown>() : undefined;
    },
    async put(key, value, ttlSeconds) {
      const response = Response.json(value, { headers: { "Cache-Control": `public, max-age=${String(ttlSeconds)}` } });
      await cache.put(cacheKey(key), response);
    },
  };
}

/** Cache-aside helper. */
export async function cached<T>(cache: JsonCache, key: string, ttlSeconds: number, compute: () => Promise<T>): Promise<T> {
  const hit = await cache.get(key);
  if (hit !== undefined) return hit as T;
  const value = await compute();
  await cache.put(key, value, ttlSeconds);
  return value;
}
