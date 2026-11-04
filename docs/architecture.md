# Cove Architecture

## Processes

```
┌──────────────────────────── Cove Desktop (Tauri 2) ────────────────────────────┐
│  UI (React + TS, WebView)  ──invoke()──▶  Core (Rust, src-tauri)               │
│                                             │  spawns & supervises sidecars    │
└─────────────────────────────────────────────┼──────────────────────────────────┘
                     stdin/stdout (NDJSON)    │    stdin/stdout (NDJSON)
              ┌───────────────────────────────┴───────────────────────┐
              ▼                                                       ▼
   cove-connectors (C# .NET 9, Native AOT)              cove-recommender (Python 3.12)
   store connectors, achievements                       recommendations

   Core ──HTTPS──▶ cove-catalog-proxy (Cloudflare Worker) ──▶ IGDB
        ──HTTPS──▶ images.igdb.com (artwork, cached on disk)
```

- The **core (Rust)** is the only process the UI talks to. It owns the library,
  launches games, loads plugins and starts/supervises the sidecars.
- The **UI (TypeScript/React)** only calls Tauri commands, never sidecars directly.
- **Plugins (JavaScript, ES modules)** run without a build step; the plugin API comes later.
- **Connectors (C#)** and **recommender (Python)** are mostly stateless worker processes.

## RPC protocol (core ↔ services)

- JSON-RPC 2.0, **one message per line** (newline-delimited JSON, UTF-8).
- Core → service via `stdin`, service → core via `stdout`.
- `stderr` is for logs only.
- Every method has:
  - TS types in `packages/shared/src/methods.ts`
  - a JSON schema in `packages/shared/schemas/<method>.schema.json`
  - mirrored types in Rust, C# (`Rpc/Messages.cs`) and Python (`rpc.py`).
- Method names: `<area>.<verb>` in camelCase, e.g. `system.ping`, `library.list`.
- Every service implements `system.ping` → `{ service, version, message }`.

## Frontend (`apps/desktop/src`)

```
api/         Data access layer: typed Tauri invoke()/listen()
stores/      Zustand: preferences (persisted), UI, downloads, running games, catalogue filter, toasts
hooks/       useCatalog (TanStack Query), useQuery, useHotkey, useHistoryNav, useDebounced
components/  ui/ (primitives), layout/ (shell, sidebar, topbar/title bar, command palette), game/
pages/       Home, Catalogue (+detail), Library (+detail), Downloads, Settings
styles/      tokens.css (raw values) + theme.css (Tailwind v4 theme)
test/        Fake core for UI tests (Tauri mockIPC); e2e.ts = Playwright entry with live core behaviour
```

- Large lists: `components/game/VirtualGrid.tsx` and `hooks/useMainVirtualizer.ts` render only the rows
  near the viewport of `#main` (TanStack Virtual). Short lists render plainly. The list view of the
  library uses spacer rows inside the table.
- Code splitting: Home and Library are in the start bundle, every other page is a lazy route.
- Errors: `RouteError` replaces React Router's developer screen; `errorMessage()` only shows texts
  written for people (from the core or `UserFacingError`).

- Routing: `createHashRouter`, behaves the same in the browser and under Tauri.
- Custom title bar: `decorations: false`. The topbar is the drag region (`data-tauri-drag-region`),
  window buttons live in `WindowControls.tsx`.
- Keyboard: `Ctrl+K` opens the command palette, `Ctrl+B` toggles the sidebar, `Alt+←/→` navigates back/forward.

## Core (`apps/desktop/src-tauri/src`)

```
lib.rs            Setup (open DB, state, plugins), command list (cove_handlers!)
commands.rs       Tauri commands (thin: validate → db/scanner/launcher)
error.rs          AppError (thiserror) → { kind, message }
models.rs         Wire types, mirror packages/shared/src/models.ts
db/               sqlx repositories: library, sessions, achievements, settings, downloads, sources
scanner.rs        Folder scan with heuristics (suggestions the user confirms)
launcher.rs       Launch plan, process-tree tracking, sessions, events
plugins.rs        Plugin discovery (manifest.json) + enabled state
catalog/          CatalogProvider trait, IGDB + RAWG clients, cache-aware service, matching, keyring, image cache
migrations/       SQL migrations (embedded via sqlx::migrate!)
```

### Launching games

1. **Steam games** always start via `steam://rungameid/<id>` (DRM, overlay, cloud saves).
2. Games with an **executable** are started directly:
   - Linux binaries, AppImages and scripts natively.
   - `.desktop` files via their `Exec` line.
   - `.exe` on Linux via Wine (`wine <exe>`) or Proton (`<proton>/proton run <exe>`, one prefix per game
     under `compat/<gameId>`). Proton is looked up in the Steam library and in `compatibilitytools.d`.
3. **GOG and Epic without an executable** use the URL schemes `goggalaxy://openGameView/<id>` and
   `com.epicgames.launcher://apps/<id>?action=launch&silent=true`.

Launch arguments are split shell-style (shlex); user environment variables override Cove's own.

### Playtime tracking

- Games are spawned in their own process group. The tracker adopts every descendant of the launched process
  and every process in that group (Linux: `/proc/<pid>/stat`), so children of launchers that exit immediately
  still count.
- Comparing process start times protects against reused PIDs.
- For URL launches with a known install folder, Cove adopts every process whose executable lives inside it.
  Without a folder Cove cannot measure (`LaunchResult.tracked = false`).
- Sessions are stored in `play_sessions` with a heartbeat every 30 s. A crash loses at most 30 s;
  open sessions are closed on the next start.
- Events: `game-state-changed` (`running` / `stopped` with session length) and `downloads-changed`.

### Folder scan (heuristics)

1. Walk up to depth 4. Hidden folders, redistributables and `Engine/` are skipped.
2. Candidates: `.exe`, ELF binaries and `.sh` with the executable bit, `.AppImage` and `.desktop`
   (`Type=Application`, not hidden).
3. Names like unins*, setup, install*, crash*, redist, updater, helper, EasyAntiCheat or BattlEye are dropped.
4. Every sub folder counts as one game. AppImages and `.desktop` files directly in the scanned folder are games
   of their own. If there are folders like `bin/` or `x86_64/`, the scanned folder itself is one game.
5. Per game, the candidate with the best score wins:
   - name similarity with the folder
   - shallow depth
   - file size
   - 64-bit path
   - on Linux: native before `.exe`
   - "launcher" in the name is penalised

### Catalogue

- `CatalogProvider` (async trait) with three implementations: **CoveProxyProvider** (default, no keys;
  see `docs/proxy.md`), **IGDB** with own Twitch keys (OAuth token refreshed automatically, 4 requests/s) and
  **RAWG** with an own API key (5 requests/s).
- Source order: Cove proxy, then own IGDB keys (both wrapped in `FallbackProvider`; any error except
  "not found" moves on to the next source), then own RAWG key, then the local cache. The proxy URL is
  compiled in from `COVE_CATALOG_PROXY_URL`; users can switch the proxy off under Settings, Katalog, Erweitert.
- Credentials are verified with the provider before they are stored in the OS keyring
  (Secret Service, macOS Keychain, Windows Credential Manager).
- Every response is cached in `catalog_cache` by request key. Fresh for 7 days; after that Cove asks the
  provider again. If the network fails, stale entries are served and marked `offline: true`; the UI shows a
  small "Offline" badge. Entries older than 60 days are pruned on startup.
- Artwork: `cove-img://localhost/<encoded url>` (Windows: `http://cove-img.localhost/…`). Only IGDB and RAWG
  image hosts are allowed. The first request downloads the file into the app cache folder, later requests
  (also offline) read it from disk. Details pages and library links prefetch their images.

### Matching library games

1. After a game is added manually or from a folder scan, the core searches the catalogue by title in the background.
2. Titles are normalized (case, symbols, edition suffixes like "GOTY", roman numerals) and scored with
   Levenshtein + Jaro-Winkler.
3. Score at least 0.95 and clearly ahead of the runner-up: metadata, genres and artwork are applied
   (`matchStatus: matched`). Executable, launch options and playtime never change.
4. Otherwise up to five candidates are stored in `match_candidates` (`matchStatus: pending`) and the user
   picks one, searches manually or rejects all (`rejected`).
5. The result is sent as the `library-match-changed` event.

### Recommendations

- `services/recommender` runs as a Tauri sidecar (`bundle.externalBin`, PyInstaller onefile). The core's
  `Sidecar` starts it at app start, checks it with `system.ping`, applies a timeout per request, restarts it
  after a crash and stops restarting after 3 starts within a minute.
- `Recommender` (`src-tauri/src/recommend.rs`) builds the input: library signals (playtime, last played,
  favourite, achievements, session count; linked games by catalogue id) and a candidate pool from cached
  catalogue pages (recommended, popular, newest).
- Home: "Für dich empfohlen" = `recommend_for_user`, "Neu entdecken" = `discover_new`; the reason is shown under
  each card. Catalogue sort "Empfohlen": the first 3 provider pages are re-ranked personally, later pages come
  in provider order.
- Without the sidecar (not built, crashed repeatedly) the UI falls back to a simple genre/rating ranking.
- Catalogue games carry `features` (themes, game modes, perspectives, keywords) for the recommender; they are
  stored with linked library games (`games.features`).

### Store accounts and achievements

- `services/connectors` (C#) runs as a second sidecar (`binaries/cove-connectors`). It is stateless: the core
  stores the session in the OS keyring and passes it with every call (`store.loginStart`, `store.loginComplete`,
  `store.refresh`, `store.library`, `store.achievements`, `store.detectLocal`).
- Login (`src-tauri/src/stores/login.rs`):
  - Steam opens the system browser with a loopback redirect to `127.0.0.1:<random port>/callback`
    (RFC 8252). The core checks `state`; the connectors verify the OpenID assertion with Steam.
  - GOG and Epic open the official page in an own window and intercept the redirect with the code.
    Epic's redirect page shows the code as JSON; a small script turns it into a `cove-auth://code` navigation.
    "Alternativer Login" lets the user paste that code.
  - The result arrives as the `connector-login-finished` event; afterwards the library is imported in the
    background (`store-sync-progress` events), new games are matched against the catalogue (paced for the
    proxy limit) and Steam achievements are read.
- Tokens are refreshed 5 minutes before they expire. A failed refresh marks the account `needsReauth`
  ("Erneut anmelden"). Logout deletes the keyring entry and optionally the imported games.
- Steam data comes through the catalogue proxy, which holds the Steam Web API key. Private profiles are
  reported with a link to the Steam privacy settings.
- Store games are keyed by `(source_id, store_game_id)`; imports refresh playtime and install data but keep
  user settings.
- Cove achievements: rules in JSON (playtime in Cove, library size, played at night, recommended games tried),
  evaluated after sessions, imports and additions. New unlocks raise `achievement-unlocked` (a small toast).
- GOG and Epic use the same public launcher clients as Heroic and Legendary. This is unofficial and may
  break when the stores change something; endpoints live in `Config/connectors.json`.

### Downloads

- `DownloadManager` (`src-tauri/src/downloads/`) runs a scheduler: it starts queued transfers up to
  `downloads.maxConcurrent`, applies the bandwidth limit from the settings to all of them together and is woken
  by every queue or settings change.
- Sources:
  - Direct links: DRM-free games, mods, patches. Optional SHA-256.
  - itch.io: official server-side API with the user's API key (OS keyring). Owned games come from
    `/profile/owned-keys`; free games are found by their page (`data.json`). The download URL is resolved
    right before each start because it expires.
  - Store games are handed to the store client (`steam://install/<id>`; on Windows the Epic and GOG Galaxy
    schemes; on Linux Heroic). Cove only watches: every 30 seconds it checks the local store manifests and
    marks the install as done.
- Transfer: a probe request (`Range: bytes=0-0`) tells whether the server supports ranges. With ranges, the file
  is split into 8 MiB chunks fetched over 4 connections into a preallocated `.part` file; finished chunks are
  saved so a restart only fetches the rest. A different `ETag`/size restarts the download. Without ranges the
  file is streamed once. Failed chunks are retried with backoff.
- After the transfer: SHA-256 check (status `verifying`), then unpacking into `<download folder>/<game>` (status
  `extracting`). Zip and 7z entries with `..` or absolute paths are skipped, tar uses `unpack_in`, symlinks in
  zips are ignored, and lost executable bits are restored for ELF files and scripts. The scanner then picks the
  executable (native first, otherwise a Windows `.exe` with Proton), the game joins the library and the
  catalogue match runs.
- Events: `downloads-changed` (full list on status changes) and `download-progress` (bytes and smoothed speed,
  every 250 ms per download). The sidebar shows the running download.

### Plugins

- A plugin is a folder with `plugin.json` (id, name, version, author, permissions, entry, apiVersion) and ES
  modules. The core (`plugins.rs`) discovers plugins in Cove's plugin folder (and, in development, the
  repository's `plugins/`), validates manifests and keeps the approval state in SQLite.
- Status: `needsApproval` (new, or an update wants more permissions), `active`, `disabled`, `invalid`. Only
  active plugins get an `entryUrl`.
- The UI starts one Web Worker per active plugin. The worker comes from a `blob:` URL so it inherits the CSP
  (`script-src 'self' cove-plugin:`; no network), removes network and storage globals and imports the entry
  module from `cove-plugin://localhost/<token>/<entry>`. The core serves files only for active plugins and
  only inside their folder; the token is random per plugin and start.
- Plugin calls go to the host as messages (`{ type: "call", method, params }`). The host checks the permission,
  validates parameters and calls the core through `src/api`; the core re-checks storage, achievements and
  `library.add`. Sidebar items with `pickFile` open the file dialog directly in the click handler.
- Plugin achievements are stored per plugin and evaluated by the same engine as Cove's own ones; they only
  count while the plugin is active.

## Resilience and updates

- `AppError::user_message` is what the UI sees. Internal errors (SQLite, I/O, JSON, keyring) are
  logged with their details and shown as a plain sentence.
- Sidecars start on first use and are restarted after a crash (at most three starts per minute).
  When one cannot run, only its features are off: Home falls back to a simple ranking, store
  sign-in and sync report that the connector service is unavailable. `system_about` reports each
  sidecar's `ServiceStatus` without starting it.
- If the database cannot be opened at start, Cove hides the window, shows a native dialog with the
  data folder and exits instead of panicking.
- Updates: `updates.rs` wraps the Tauri updater. `latest.json` comes from the GitHub release;
  packages are verified with the public key in `tauri.conf.json` before they are installed. Only
  Windows and the AppImage update themselves; `.deb`/`.rpm` installs get a hint instead.

## Open points

- Playtime tracking on Windows via Job Objects instead of polling
- Plugins: zip/URL install and update checks
- E2E tests against the real Tauri app (WebDriver via `tauri-driver`); Playwright cannot drive the webview
