/** Worker bindings (see wrangler.toml). Secrets are set with `wrangler secret put`. */
export interface Env {
  /** Twitch app access token (one key, refreshed before expiry). */
  TOKENS: KVNamespace;
  /** Serialises all requests to IGDB (global 4 req/s). */
  IGDB_GATE: DurableObjectNamespace;
  /** Per-client-IP limit (60 req/min). */
  IP_LIMIT: RateLimit;
  IGDB_CLIENT_ID: string;
  IGDB_CLIENT_SECRET: string;
  /** Steam Web API key (https://steamcommunity.com/dev/apikey). */
  STEAM_API_KEY: string;
}
