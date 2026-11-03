/**
 * Talks to Twitch (token) and IGDB. The token lives in KV and is renewed
 * 10 minutes before it expires; every IGDB request waits for a slot from the
 * global gate and is retried on HTTP 429.
 */
import type { Env } from "./env.ts";

export const TOKEN_URL = "https://id.twitch.tv/oauth2/token";
export const API_BASE = "https://api.igdb.com/v4";
const TOKEN_KEY = "igdb:token";
const RENEW_BEFORE_S = 600;
const MAX_RETRIES = 3;

export class UpstreamError extends Error {
  constructor(
    readonly status: number,
    message: string,
  ) {
    super(message);
    this.name = "UpstreamError";
  }
}

interface StoredToken {
  accessToken: string;
  /** Unix seconds. */
  expiresAt: number;
}

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

export class IgdbClient {
  /**
   * Never called as `this.fetchFn(...)`: the Workers runtime rejects `fetch`
   * invoked with a foreign `this` ("Illegal invocation").
   */
  private readonly fetchFn: typeof fetch;

  constructor(
    private readonly env: Env,
    fetchFn: typeof fetch = (input, init) => fetch(input, init),
    private readonly now: () => number = Date.now,
  ) {
    this.fetchFn = (input, init) => fetchFn(input, init);
  }

  private nowSeconds(): number {
    return Math.floor(this.now() / 1000);
  }

  async token(forceRefresh = false): Promise<string> {
    if (!this.env.IGDB_CLIENT_ID || !this.env.IGDB_CLIENT_SECRET) {
      throw new UpstreamError(500, "IGDB credentials are not configured (wrangler secret put).");
    }
    if (!forceRefresh) {
      const stored = await this.env.TOKENS.get<StoredToken>(TOKEN_KEY, "json");
      if (stored && stored.expiresAt - RENEW_BEFORE_S > this.nowSeconds()) return stored.accessToken;
    }
    const url = new URL(TOKEN_URL);
    url.searchParams.set("client_id", this.env.IGDB_CLIENT_ID);
    url.searchParams.set("client_secret", this.env.IGDB_CLIENT_SECRET);
    url.searchParams.set("grant_type", "client_credentials");
    const response = await this.fetchFn(url, { method: "POST" });
    if (!response.ok) throw new UpstreamError(502, `Twitch token request failed (HTTP ${String(response.status)})`);
    const body: { access_token: string; expires_in: number } = await response.json();
    const token: StoredToken = { accessToken: body.access_token, expiresAt: this.nowSeconds() + body.expires_in };
    // KV entries need a TTL of at least 60 s.
    const ttl = Math.max(60, body.expires_in - RENEW_BEFORE_S);
    await this.env.TOKENS.put(TOKEN_KEY, JSON.stringify(token), { expirationTtl: ttl });
    return token.accessToken;
  }

  /** Waits for the next global IGDB slot. */
  private async acquireSlot(): Promise<void> {
    const gate = this.env.IGDB_GATE.get(this.env.IGDB_GATE.idFromName("igdb"));
    const response = await gate.fetch("https://gate/acquire", { method: "POST" });
    const { waitMs }: { waitMs: number } = await response.json();
    if (response.status === 503) throw new UpstreamError(503, "IGDB queue is full, try again shortly.");
    if (waitMs > 0) await sleep(waitMs);
  }

  async query<T>(endpoint: string, body: string): Promise<T> {
    let refreshed = false;
    for (let attempt = 0; ; attempt += 1) {
      const token = await this.token();
      await this.acquireSlot();
      const response = await this.fetchFn(`${API_BASE}/${endpoint}`, {
        method: "POST",
        headers: {
          "Client-ID": this.env.IGDB_CLIENT_ID,
          Authorization: `Bearer ${token}`,
          Accept: "application/json",
        },
        body,
      });
      if (response.ok) return response.json();
      if (response.status === 401 && !refreshed) {
        refreshed = true;
        await this.token(true);
        continue;
      }
      if (response.status === 429 && attempt < MAX_RETRIES) {
        await sleep(500 * (attempt + 1));
        continue;
      }
      throw new UpstreamError(response.status === 429 ? 503 : 502, `IGDB answered HTTP ${String(response.status)}`);
    }
  }
}
