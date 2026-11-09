# Cove: Notes for Claude

Cove is a universal desktop game launcher for Linux and Windows (macOS later).
It brings games from every store and local games together in one calm, tidy library.
Architecture details: `docs/architecture.md`.

## Languages and roles

| Area | Language | Location | Role |
|---|---|---|---|
| Core | **Rust** (Tauri 2) | `apps/desktop/src-tauri` | Library, game launching, process/sidecar management, plugin host, the only bridge to the UI |
| UI | **TypeScript** + React + Vite | `apps/desktop/src` | Interface; talks to the core only through Tauri commands |
| Plugins | **JavaScript** (ES modules, no build) | `plugins/` | Third-party extensions |
| Store connectors & achievements | **C#** .NET 9, Native AOT | `services/connectors` | Steam/GOG/Epic/… integration, achievements |
| Recommendations | **Python** 3.12 (uv, numpy, scikit-learn) | `services/recommender` | Local hybrid recommender, Tauri sidecar (PyInstaller binary) |
| Catalogue proxy | **TypeScript** (Cloudflare Worker, Hono, zod) | `services/catalog-proxy` | Fixed API in front of IGDB so users need no keys; holds the IGDB secrets |
| Shared contracts | TypeScript + JSON Schema | `packages/shared` | Types and schemas for **all** RPC messages |

**Communication:** JSON-RPC 2.0 over stdio, one JSON message per line. stdout is for RPC only, logs go to stderr.
When you add or change an RPC method, update **in the same step**:
the TS types (`packages/shared/src/methods.ts`), the JSON schema (`packages/shared/schemas/`) and the mirrored types in Rust, C# and Python.

## Design rules (mandatory for every UI change)

- **Clean, minimal, calm.** **Not** futuristic: no neon colours, no glow, no glassmorphism/blur, no gradients, no sci-fi fonts.
- It should feel like Linear, Notion or Apple Music: lots of white space and clear typography.
- **Font:** Inter (`@fontsource-variable/inter`, bundled locally, no CDN). Numbers are always tabular (`font-variant-numeric: tabular-nums`, class `.num`).
- **Font sizes:** only 12 / 14 / 16 / 20 / 28 px (`text-xs` … `text-xl`).
- **Colours:** neutral greys and **exactly one** calm accent colour (muted blue `#3B6FD8`). Cove is **dark only** (no light mode, no theme switch); `data-theme="dark"` is always set, also in `index.html`.
- **Spacing** on a 4px grid. **Radius** 8px, cards 12px.
- **Separation** through subtle 1px borders (`border-border`), **no shadows**.
- **Icons:** `lucide-react`, 18px, stroke width 1.75.
- **Animations** only subtle: 150 to 200 ms ease-out. Always respect `prefers-reduced-motion`.
- All raw values live in `apps/desktop/src/styles/tokens.css` (`--cove-*`, the dark values under `[data-theme="dark"]`, accent via `data-accent`). `styles/theme.css` maps them to Tailwind utilities (`bg-surface`, `text-fg-muted`, `border-border`, `bg-accent` …).
- In `theme.css`, Tailwind's default palette, shadows, blur and every font size except the five allowed ones are **switched off**. Classes like `bg-pink-500`, `shadow-lg` or `text-4xl` generate no CSS. Do not switch them back on. If a token is missing, add it to `tokens.css` and `theme.css`.
- **No hard-coded colours in components.** ESLint forbids hex, `rgb()` and `hsl()` literals in `apps/desktop/src`.
- Icon size and stroke width are set globally by `LucideProvider` in `App.tsx`. Don't pass `size` or `strokeWidth` to icons.
- Always use `GameCover` for game artwork. Without an image URL it renders a calm, flat placeholder.
- Every page needs a skeleton loading state (no spinners), an empty state with friendly text (`EmptyState`) and an error state with a retry action.
- Focus states are always visible (`:focus-visible`). Everything must be usable with the keyboard.
- **Contrast WCAG 2.1 AA:** text at least 4.5:1 on every surface it can sit on (`bg`, `bg-sidebar`, `hover`, `active`), input outlines and switch tracks at least 3:1 (`border-input`). Check new tokens with all five accents; `e2e/a11y.e2e.ts` runs axe on every page.
- Use only 4px-grid spacing utilities (`1`, `2`, `3` …), no half steps (`1.5`, `2.5`). Icons stay 18px, also next to small text.
- Game covers without alt text are decoration (`aria-hidden`); give their link an `aria-label` with the title.
- **UI languages: German and English.** Never write UI text into a component: add it to `src/i18n/de.ts` **and** `src/i18n/en.ts` (same keys; `i18n.test.ts` fails on missing keys or umlauts in English) and read it with `useMessages()` (components) or `messages()` (other code). Texts with values are functions (`(title: string) => …`). Label tables use `localized()`. Formats come from `lib/format.ts` (locale follows the language). The main navigation labels Home, Catalogue, My Library, Downloads, Settings stay English in both languages on purpose.
- Cove's own logo is `components/ui/CoveLogo.tsx` (light square, dark rings, drawn with theme tokens); the app icons are generated from `src-tauri/icons/icon.svg` with `npx tauri icon <1024px png> -o src-tauri/icons` (delete the android/ios folders it creates).
- Store logos (`components/ui/StoreIcon.tsx`, Simple Icons, CC0) are drawn in the current text colour at 18px, never in brand colours.

## Code rules

- **TypeScript:** `strict` plus `noUncheckedIndexedAccess` and `exactOptionalPropertyTypes`. **No `any`**: use `unknown` and type guards. ESLint (`typescript-eslint` strictTypeChecked) must pass without errors.
- **JavaScript (plugins):** ES modules, JSDoc types, ESLint recommended.
- **Rust:** `cargo clippy --all-targets -- -D warnings` (pedantic enabled) without warnings, `cargo fmt`. No `unsafe`. No `unwrap()` outside tests; handle errors as `Result`.
- **C#:** `Nullable` enabled, `TreatWarningsAsErrors`. Everything must be AOT compatible, so no reflection-based serialization: JSON only via the source-generated `RpcJsonContext`. `dotnet format`.
- **Python:** 3.12, `ruff check` + `ruff format`, `mypy --strict`. No `Any` in signatures (ruff `ANN401`).
- **Frontend data access:** The UI gets data **only** through `apps/desktop/src/api/*` (`import { api } from "…/api"`). All core calls go through the typed `call()` / `subscribe()` in `api/transport.ts`. Outside `src/api` (and `src/test`), ESLint forbids imports from `src/mocks`, `@tauri-apps/api/core` and `@tauri-apps/api/event`.
- **No mocks** in the app anymore: everything comes from the Rust core. UI tests use the fake core in `src/test/fakeCore.ts` (Tauri `mockIPC`).
- **Server state:** catalogue data uses TanStack Query (`hooks/useCatalog.ts`, client in `lib/queryClient.ts`). Older views use the small `useQuery` hook; `invalidateQueries()` refreshes both.
- **Catalogue:** sources in fallback order: Cove proxy (`CoveProxyProvider`, URL from `COVE_CATALOG_PROXY_URL` at build time, default in `src-tauri/.cargo/config.toml`), own IGDB keys, own RAWG key, then the local cache. All behind the Rust trait `CatalogProvider` (`src-tauri/src/catalog/`).
- **Catalogue proxy (Worker):** never add generic pass-through routes; every IGDB query is built in the Worker from validated parameters. Secrets only via `wrangler secret put`. Never call `fetch` as a method of another object (workerd throws "Illegal invocation"). Responses must match the `Proxy*` types in `packages/shared`. Deploy guide: `docs/proxy.md`. Credentials only in the OS keyring (`catalog/secrets.rs`), never in SQLite or the UI. Always go through `SecretStore`/`KeyringStore::app()`, never `keyring::Entry` directly: the Secret Service backend starts its own tokio runtime per call and panics on an async worker, so `KeyringStore` runs every call on a plain thread (`off_runtime`). Windows' Credential Manager holds only about 1280 characters per entry, so `KeyringStore` splits longer values (Epic sessions) into parts `<key>#0…` (`chunked`); CI runs `real_keyring_roundtrip` on Windows. Unit tests use `MemoryStore`; after keyring or login changes run `cargo test real_keyring -- --ignored` (real keyring under the separate service `app.cove.desktop.test`, store and itch.io logins through the IPC path; needs the built connectors, e.g. `COVE_CONNECTORS_CMD=binaries/cove-connectors-x86_64-unknown-linux-gnu`). Provider responses are cached in `catalog_cache` (TTL 7 days, stale entries served when offline). Images go through the `cove-img://` scheme and are cached on disk; `ImageRef` rewrites URLs on serialization, so Rust code and the database always hold the original URLs. Steam games use the portrait artwork `library_600x900_2x.jpg` as cover (never the 32×32 icon); when it is missing the image cache falls back to the 1x portrait, the header and finally the header URL from the Steam store API (`images.rs::steam_fallback`). A catalogue match without artwork keeps the existing cover.
- **Recommender sidecar:** started by `src-tauri/src/sidecar.rs` (health check via `system.ping`, request timeout, restart after crash, at most 3 starts per minute) and fed by `src-tauri/src/recommend.rs`. It is stateless: the core sends library signals (`RecommenderLibraryGame`) and catalogue features (`RecommenderGame`) with every request. When it is unavailable, the UI falls back to a simple ranking; never make it a hard dependency. After changing the Python code run `npm run build:recommender -w @cove/desktop`, then `cargo test sidecar_contract` checks the real binary.
- **Plugins (step 7):** guide in `docs/plugins.md`, API types in `packages/shared/src/plugin-api.ts` (versioned, `PLUGIN_API_VERSION`).
  - Plugins run in a dedicated **blob** Web Worker (`src/plugins/worker.ts`, imported with `?worker&inline` so it inherits the CSP) and only talk through messages (`src/plugins/runtime.ts` ↔ `src/plugins/host.ts`). Never give a plugin access to the DOM, Tauri, the network or another plugin.
  - Every API method maps to one permission (`METHOD_PERMISSIONS`). The host checks it; the core re-checks storage, achievements and `library.add` (`Plugins::require`). A new permission needs: the `PluginPermission` type, the Rust enum, `METHOD_PERMISSIONS`, `PERMISSION_LABELS` (UI text) and the docs.
  - Approval state lives in the `plugins` table: a plugin runs only when `granted` covers what `plugin.json` asks for. Files are served via `cove-plugin://localhost/<token>/<path>` (random token per plugin and start; `plugins.rs::resolve_file` refuses other plugins, `..` and symlinks).
  - Plugin data is JSON in `plugin_storage` (limits in `plugins.rs`), plugin achievements in `plugin_achievements`.
  - Example plugins in `/plugins` are tested twice: `node --test` with a fake API, and `src/plugins/host.test.ts` through the real host and runtime.
- **Downloads (step 6):** the engine lives in `src-tauri/src/downloads/` (`DownloadManager`). Only legal sources:
  - Direct links (DRM-free games, mods, patches) and itch.io through its official API (games the user owns, or free ones). The itch.io API key lives **only** in the OS keyring (`itch-api-key`); itch download URLs are resolved right before each start and never stored.
  - Store games (Steam/GOG/Epic) are **never** downloaded by Cove: `downloads_enqueue` hands the install to the store client (`downloads/handoff.rs`, Heroic on Linux for GOG/Epic) and the status `handedOff` is finished once the local manifest shows the game.
  - Parallel range chunks (`transfer.rs`) with resume (finished chunks in `downloads.chunks_done`), one shared bandwidth limit (`limiter.rs`, from the settings), SHA-256 check, unpacking (zip, 7z, tar, tar.gz; unsafe paths are skipped) and executable detection via the scanner. Progress goes out as `download-progress` at most 4 times per second; status changes as `downloads-changed`.
  - All state is in SQLite: interrupted transfers are queued again at start. Pause and cancel must react at once (`Control` watch channel), not only after the next network packet.
  - Tests use `wiremock` servers with real range support (`transfer::tests::RangeServer`); never download from the internet in tests.
- **Store accounts (step 5):** the C# connectors run as a second sidecar (`resolve_connectors()`, errors via `stores::CONNECTOR_ERRORS`). The core keeps the session and passes it per call; the connectors do the token exchange, refresh and store API calls.
  - **Never** see or store passwords. Store sessions (access + refresh token) live **only** in the OS keyring under `store-session:<store>` (`src-tauri/src/stores/`), never in SQLite, logs or files. `StoreSession`'s `Debug` redacts tokens; keep it that way.
  - Steam: system browser + loopback redirect on `127.0.0.1:<random port>/callback` with a `state` check (`src-tauri/src/auth/`), OpenID verified with `check_authentication`. GOG/Epic: own webview window, navigation intercepted (`auth/webview.rs`); Epic also has the "Alternativer Login" with a pasted code.
  - The Steam Web API key lives **only** in the catalogue proxy (Worker secret `STEAM_API_KEY`); the connectors call `/v1/steam/*`.
  - Store endpoints and client ids live in one file: `services/connectors/src/Cove.Connectors/Config/connectors.json`.
  - Connector failures carry `error.data.kind` (`authFailed`, `privateProfile`, `unavailable`, `network`, `invalidRequest`); the core maps them to `AppError` and marks accounts `needsReauth` when a refresh fails.
  - Cove's own achievements are JSON (`src-tauri/resources/cove-achievements.json`, plus `achievements.json` in plugin folders) and evaluated in `src-tauri/src/cove_achievements.rs`.
  - After changing the C# code run `npm run build:connectors -w @cove/desktop` (`build:sidecar` builds both sidecars), then `cargo test sidecar_contract`.
- **Cached `Game` data:** when its shape changes, bump `CACHE_VERSION` in `src-tauri/src/catalog/mod.rs` and in `services/catalog-proxy/src/cache.ts`.
- **External APIs in tests:** never call real services. Use `wiremock` with the providers' `with_endpoints`/`with_base` constructors and `Endpoints` for the service.
- **New Tauri command:** always change all five places together:
  1. function in `src-tauri/src/commands.rs`
  2. entry in the `cove_handlers!` macro in `lib.rs`
  3. signature in `packages/shared/src/commands.ts` (`CoveCommands`, camelCase arguments)
  4. wrapper in `src/api/*`
  5. handler in the test fake `src/test/fakeCore.ts`

  Also cover the command in `src-tauri/src/ipc_tests.rs` with the real JSON payload.
- **Errors** from Rust are `AppError` (thiserror) and reach the frontend as `{ kind, message }`, there as `CoveError`. The `message` is user-facing text in the UI language (`AppError::user_message`): database, file, keyring and JSON errors get a plain sentence, their technical text only goes to stderr. Never put paths, URLs, OS error codes or stack traces into a message. A new `kind` also goes into `AppErrorKind` in `models.ts`.
  - In the UI always show errors through `errorMessage()` (`stores/toasts.ts`): it passes `CoveError` and `UserFacingError` through and replaces anything else (a programming error) with a generic sentence. Page crashes end in `RouteError`, never in React Router's developer screen.
  - Every text that reaches the UI is written with `tr!("Deutsch", "English", args…)` (`src-tauri/src/i18n.rs`, works like `format!`); log output stays as it is. The language comes from `settings.general.language` (new installs: system language). Tests run German; `i18n::in_english` / `set_thread_english` switch one test thread only. Sidecars get the language via `system.setLanguage` (sent by `Sidecar::call`); C# uses `Texts.T(de, en)`, Python `texts.t(de, en)`.
  - Sidecar processes start with `CREATE_NO_WINDOW` on Windows (no console window).
  - Sidecars are never required: when one is missing or crashes, the app keeps working and only its features are off (`SidecarStatus`, shown in Settings → Über Cove). A broken database at start shows a dialog (`report_startup_failure`), not a panic.
- **Performance (step 8):** lists that can hold thousands of games use `VirtualGrid` / `useMainVirtualizer` (TanStack Virtual on `#main`; plain rendering up to `VIRTUALIZE_FROM` items). Images load lazily (`loading="lazy"`, `decoding="async"`). Pages other than Home and Library are lazy routes in `router.tsx`. The core logs `Oberfläche geladen nach … ms` once per start; keep it well under 2 s (currently about 0.6 s with 2500 games).
- **Updates:** `src-tauri/src/updates.rs` (Tauri updater plugin, commands `updates_check` / `updates_install`, event `update-progress`). Releases come from `.github/workflows/release.yml` on a `v*` tag; `latest.json` and signatures are uploaded there. The private signing key lives only in `~/.tauri/cove-updater.key` and the GitHub secret `TAURI_SIGNING_PRIVATE_KEY`; never commit it. `tauri.release.conf.json` switches on `createUpdaterArtifacts` for those builds only.
- **E2E tests:** Playwright in `apps/desktop/e2e/*.e2e.ts`, against a production build of `e2e.html` (`src/test/e2e.ts`: the fake core plus the live behaviour of the Rust core). New main flows get an E2E test; README screenshots come from `COVE_SCREENSHOTS=1 npx playwright test screenshots`.
- **Rust types** in `src-tauri/src/models.rs` mirror `packages/shared/src/models.ts`. `contract_tests.rs` validates serialized values against `models.schema.json`.
- **Database:** SQLite via sqlx with runtime queries (no `query!` macro, so no `DATABASE_URL` needed). Schema changes always come as a **new** file `src-tauri/migrations/NNNN_*.sql`; never edit existing migrations. DB functions get tests using `db::open_in_memory()`.
- **Data types** (Game, LibraryEntry, Achievement, Download, CatalogFilter …) live in `packages/shared/src/models.ts`. After every change regenerate the schema with `npm run schemas -w @cove/shared`. One test checks the schema is up to date, another that mock and fake data match it.
- **State:** Zustand stores live in `apps/desktop/src/stores`; load server data with `useQuery` (`hooks/useQuery.ts`). Call `invalidateQueries()` after mutations.
- Tests live next to the code (TS/JS: `*.test.ts(x)` / `*.test.js`; Rust: `#[cfg(test)]`) or in `tests/` (C#, Python).
- Install dependencies with the package manager (`npm i`, `cargo add`, `uv add`, `dotnet add package`). Never guess versions.
- TypeScript stays pinned to `~6.0` until `typescript-eslint` supports TS 7.

## Commands

Requirements: Node 22+, Rust (stable, with `clippy` and `rustfmt`), .NET SDK 9, `uv`. On Linux also the Tauri system packages (webkit2gtk-4.1, gtk3, libsoup3). AOT publishing additionally needs `clang` and `zlib-devel`.

### Root (TS/JS, all workspaces)
```sh
npm install              # once
npm run dev              # starts the desktop app (Tauri + Vite)
npm run lint             # ESLint + tsc for all workspaces
npm run test             # Vitest (shared, desktop) + node:test (plugins)
npm run check            # lint + test
npm run test:e2e -w @cove/desktop # Playwright (once: npx playwright install chromium in apps/desktop)
npm run schemas -w @cove/shared   # regenerate the JSON schema from models.ts
```

### Desktop: UI + Rust core (`apps/desktop`)
```sh
npm run tauri:dev -w @cove/desktop     # app with hot reload
npm run dev -w @cove/desktop           # frontend only in the browser (http://localhost:1420), window buttons inactive
npm run tauri:build -w @cove/desktop   # release bundle
cd apps/desktop/src-tauri
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo test
```

CI: `.github/workflows/ci.yml` (lint + tests for all five languages, E2E, Rust on Linux and Windows) and `release.yml` (AppImage, .deb, .rpm, MSI, NSIS; draft release on `v*` tags).

App data (SQLite `cove.db`, Proton prefixes under `compat/`, plugins under `plugins/`) lives in Tauri's app data folder, on Linux `~/.local/share/app.cove.desktop/`. In the browser (`npm run dev -w @cove/desktop`) only the catalogue works; everything from the core shows "only available in the desktop app".

### Connectors: C# (`services/connectors`)
```sh
dotnet build
dotnet test
dotnet format --verify-no-changes
dotnet run --project src/Cove.Connectors
dotnet publish src/Cove.Connectors -c Release -r linux-x64   # Native AOT (win-x64 on Windows)
npm run build:connectors -w @cove/desktop   # sidecar binary for Tauri (Native AOT with clang, else self-contained; skipped when unchanged)
```

### Recommender: Python (`services/recommender`)
```sh
npm run build:recommender -w @cove/desktop   # PyInstaller binary for Tauri (skipped when unchanged)
uv sync
uv run cove-recommender
uv run ruff check . && uv run ruff format --check .
uv run mypy
uv run pytest
```

### Catalogue proxy (`services/catalog-proxy`)
```sh
npm test -w @cove/catalog-proxy            # vitest with fake bindings
npm run check:deploy -w @cove/catalog-proxy # wrangler dry-run: validates bindings + bundle
npx wrangler dev                           # local workerd (needs .dev.vars, see .dev.vars.example)
```

### Plugins (`plugins`)
```sh
node --test "plugins/**/*.test.js"
```

Smoke test for a service:
`echo '{"jsonrpc":"2.0","id":1,"method":"system.ping","params":{"message":"hi"}}' | <service command>`

## Working rule

**After every change, run lint and tests for all affected sub-projects before saying "done".**
If something fails, fix it or state the failure explicitly. If a tool is missing locally, say so instead of claiming success.
