/** In-memory stand-ins for Cloudflare bindings and the IGDB/Twitch network. */
import type { JsonCache } from "../src/cache.ts";
import type { Env } from "../src/env.ts";
import { IgdbGate } from "../src/gate.ts";

export class FakeKv {
  readonly data = new Map<string, string>();
  readonly puts: { key: string; ttl: number | undefined }[] = [];
  get(key: string, type?: "json"): Promise<unknown> {
    const raw = this.data.get(key);
    if (raw === undefined) return Promise.resolve(null);
    return Promise.resolve(type === "json" ? (JSON.parse(raw) as unknown) : raw);
  }
  put(key: string, value: string, options?: { expirationTtl?: number }): Promise<void> {
    this.data.set(key, value);
    this.puts.push({ key, ttl: options?.expirationTtl });
    return Promise.resolve();
  }
}

export class FakeCache implements JsonCache {
  readonly data = new Map<string, unknown>();
  readonly ttls = new Map<string, number>();
  get(key: string): Promise<unknown> {
    return Promise.resolve(this.data.get(key));
  }
  put(key: string, value: unknown, ttl: number): Promise<void> {
    this.data.set(key, structuredClone(value));
    this.ttls.set(key, ttl);
    return Promise.resolve();
  }
}

export class FakeLimiter {
  readonly counts = new Map<string, number>();
  constructor(private readonly max = 1000) {}
  limit({ key }: { key: string }): Promise<{ success: boolean }> {
    const n = (this.counts.get(key) ?? 0) + 1;
    this.counts.set(key, n);
    return Promise.resolve({ success: n <= this.max });
  }
}

/** Durable Object namespace with a single real `IgdbGate` instance. */
export function fakeGateNamespace(now: () => number = Date.now) {
  const gate = new IgdbGate({} as DurableObjectState, {}, now);
  let calls = 0;
  return {
    get calls() {
      return calls;
    },
    idFromName: (name: string) => ({ name }),
    get: () => ({
      fetch: () => {
        calls += 1;
        return Promise.resolve(gate.fetch());
      },
    }),
  };
}

export interface FakeEnvOptions {
  ipLimit?: number;
  secrets?: boolean;
}

export function fakeEnv(options: FakeEnvOptions = {}) {
  const kv = new FakeKv();
  const gate = fakeGateNamespace();
  const limiter = new FakeLimiter(options.ipLimit);
  const env = {
    TOKENS: kv,
    IGDB_GATE: gate,
    IP_LIMIT: limiter,
    IGDB_CLIENT_ID: options.secrets === false ? "" : "cid",
    IGDB_CLIENT_SECRET: options.secrets === false ? "" : "secret",
  } as unknown as Env;
  return { env, kv, gate, limiter };
}

export interface IgdbCall {
  endpoint: string;
  body: string;
  headers: Headers;
}

/**
 * Fake network: Twitch token endpoint + IGDB endpoints answered by `igdb`.
 * Records every call. `igdb` may return a Response to simulate errors.
 */
export function fakeNetwork(igdb: (call: IgdbCall) => unknown) {
  const calls: IgdbCall[] = [];
  let tokenRequests = 0;
  const fetchFn = ((input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
    const url = new URL(typeof input === "string" ? input : input instanceof URL ? input.href : input.url);
    if (url.hostname === "id.twitch.tv") {
      tokenRequests += 1;
      return Promise.resolve(Response.json({ access_token: `tok${String(tokenRequests)}`, expires_in: 5_000_000 }));
    }
    const call: IgdbCall = {
      endpoint: url.pathname.replace("/v4/", ""),
      body: typeof init?.body === "string" ? init.body : "",
      headers: new Headers(init?.headers),
    };
    calls.push(call);
    const result = igdb(call);
    return Promise.resolve(result instanceof Response ? result : Response.json(result));
  }) as typeof fetch;
  return {
    fetchFn,
    calls,
    get tokenRequests() {
      return tokenRequests;
    },
  };
}

export const WITCHER = {
  id: 1942,
  name: "The Witcher 3: Wild Hunt",
  summary: "An open-world RPG set in a dark fantasy world of monsters. More text follows here.",
  genres: [12, 31],
  themes: [1],
  platforms: [6, 3, 48],
  first_release_date: 1_431_993_600,
  involved_companies: [
    { company: { name: "CD Projekt RED" }, developer: true, publisher: false },
    { company: { name: "CD Projekt" }, developer: false, publisher: true },
  ],
  total_rating: 92.6,
  total_rating_count: 3000,
  cover: { image_id: "co1wyy" },
  screenshots: [{ image_id: "sc1" }],
  artworks: [{ image_id: "ar1" }],
};
