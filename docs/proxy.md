# Cove catalogue proxy

`services/catalog-proxy` is a Cloudflare Worker (TypeScript, Hono, zod) that lets Cove show IGDB game data
without users creating their own API keys. It holds the IGDB credentials, exposes a small fixed API and
caches answers.

## What it does

| Endpoint | Purpose | Cache |
|---|---|---|
| `GET /v1/search?q=` | Search by title (2 to 100 characters) | 24 h |
| `GET /v1/browse?genre=&platform=&year=&rating=&q=&sort=&page=` | Paged catalogue (24 per page) | 24 h |
| `GET /v1/game/:id` | Details of one IGDB game | 7 days |
| `GET /v1/similar/:id` | Similar games | 7 days |
| `POST /v1/match` `{ "title": "…" }` | Best catalogue candidates for a title, scored | 24 h |
| `GET /v1/steam/profile/:steamid` | Steam name and whether the profile is public | 1 h |
| `GET /v1/steam/owned/:steamid` | Owned games with playtime (`private: true` when hidden) | 1 h |
| `GET /v1/steam/achievements/:steamid/:appid` | Player progress merged with names, icons and global rarity | 1 h (schema and rarity 7 days) |
| `GET /v1/health` | Liveness check | none |

- **No pass-through.** Clients only send parameters; the Worker builds every IGDB (Apicalypse) query itself.
  Any other path answers `404`.
- **Validation** with zod: `genre` and `platform` are comma separated lists of Cove values
  (`rpg,horror`, `linux,windows`), `year` is `2020` or `2018-2021`, `rating` 0 to 100,
  `sort` one of `recommended | popular | newest | title`, `page` 0 to 200.
- **Responses** use the types `ProxyPage`, `ProxyGame`, `ProxyMatchResponse` and `ProxyError` from
  `packages/shared/src/models.ts`. Errors always look like `{ "error": { "code", "message" } }`.
- **Images are not proxied.** Cover and screenshot URLs point to `images.igdb.com`; the app downloads them
  directly and caches them on disk.
- **Twitch token**: client-credentials flow, stored in Workers KV and renewed 10 minutes before it expires.
- **Rate limits**:
  - towards IGDB: at most 4 requests per second for the whole Worker, enforced by a single Durable Object
    (`IgdbGate`) that hands out time slots. HTTP 429 from IGDB is retried up to 3 times; a full queue
    answers `503`.
  - per client IP: 60 requests per minute (Workers Rate Limiting binding), answered with `429`.
- **Caching** uses the Cache API (per data centre, no write limits). KV only stores the token, because KV's
  free tier allows about 1,000 writes per day.

## Deploy

Requirements: a Cloudflare account (the free plan is enough) and a Twitch application for IGDB.

1. **Create the Twitch application.** At <https://dev.twitch.tv/console> choose *Register Your Application*,
   any name, OAuth redirect `http://localhost`, category *Application Integration*. Copy the
   **Client ID** and create a **Client Secret**.

2. **Log in to Cloudflare** (from the repository root):

   ```sh
   cd services/catalog-proxy
   npx wrangler login
   ```

3. **Create the KV namespace** for the token and put its id into `wrangler.toml`
   (replace `REPLACE_WITH_KV_NAMESPACE_ID`):

   ```sh
   npx wrangler kv namespace create TOKENS
   ```

4. **Store the credentials as Worker secrets.** They never go into the repository or the app:

   ```sh
   npx wrangler secret put IGDB_CLIENT_ID
   npx wrangler secret put IGDB_CLIENT_SECRET
   npx wrangler secret put STEAM_API_KEY      # https://steamcommunity.com/dev/apikey (for the Steam endpoints)
```

5. **Check and deploy:**

   ```sh
   npm run check:deploy   # wrangler deploy --dry-run: validates bindings and bundles
   npm run deploy
   ```

   Wrangler prints the URL, e.g. `https://cove-catalog-proxy.<account>.workers.dev`.

6. **Smoke test:**

   ```sh
   curl https://cove-catalog-proxy.<account>.workers.dev/v1/health
   curl "https://cove-catalog-proxy.<account>.workers.dev/v1/search?q=witcher"
   ```

7. **Point Cove at the proxy.** The URL is compiled into the app (`build.rs`). Put it into
   `apps/desktop/src-tauri/.cargo/config.toml` (`[env] COVE_CATALOG_PROXY_URL`), then every
   `npm run dev` and `npm run tauri:build` uses it. The URL is not a secret: every Cove build contains it.

   To try another Worker without editing the file, set the variable for one run; it wins over the file:

   ```sh
   COVE_CATALOG_PROXY_URL=http://127.0.0.1:8787 npm run dev
   ```

   With an empty URL the app has no Cove service and only uses own keys (Settings, Katalog, Erweitert).

## Local development

```sh
cd services/catalog-proxy
cp .dev.vars.example .dev.vars   # put real or dummy credentials in it; never commit it
npx wrangler dev                 # http://127.0.0.1:8787, runs in workerd with local KV and Durable Objects
npm test                         # unit tests (fake bindings, fake IGDB)
```

Point a development build at it with `COVE_CATALOG_PROXY_URL=http://127.0.0.1:8787 npm run dev`.

Wrangler sends anonymous usage telemetry; set `WRANGLER_SEND_METRICS=false` to turn it off.

## How the app uses it

Fallback order in the core (`src-tauri/src/catalog`):

1. **Cove proxy** (`CoveProxyProvider`), if the build has a proxy URL and the user did not switch it off.
2. **Own IGDB keys** (Twitch client id and secret in the OS keyring), if set in Settings under *Erweitert*.
3. **Local SQLite cache**: when every source fails because of the network, cached answers are shown with an
   "Offline" badge.

A user's own RAWG key is used only when neither the proxy nor own IGDB keys are available.

## Operations

- **Rotate the IGDB secret:** create a new secret in the Twitch console, then
  `npx wrangler secret put IGDB_CLIENT_SECRET`. The cached token keeps working until it expires; to switch
  immediately, delete the `igdb:token` key: `npx wrangler kv key delete --binding TOKENS igdb:token`.
- **Logs:** `npx wrangler tail`, or the Workers dashboard (observability is enabled in `wrangler.toml`).
- **Limits:** the per-IP limit is evaluated per Cloudflare location and is meant against abuse, not as an
  exact quota. Change it in `wrangler.toml` (`[[ratelimits]]`, period must be 10 or 60 seconds).
- **IGDB terms:** IGDB data is free for non-commercial use with attribution. Cove shows
  "Game data provided by IGDB.com" in Settings, *Über Cove*. Check the current terms at
  <https://api-docs.igdb.com/> before commercial use.
