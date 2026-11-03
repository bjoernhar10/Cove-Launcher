# Cove — Notes for Claude

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
| Recommendations | **Python** 3.12 (uv) | `services/recommender` | Recommendation engine |
| Shared contracts | TypeScript + JSON Schema | `packages/shared` | Types and schemas for **all** RPC messages |

**Communication:** JSON-RPC 2.0 over stdio, one JSON message per line. stdout is for RPC only, logs go to stderr.
When you add or change an RPC method, update **in the same step**:
the TS types (`packages/shared/src/methods.ts`), the JSON schema (`packages/shared/schemas/`) and the mirrored types in Rust, C# and Python.

## Design rules (mandatory for every UI change)

- **Clean, minimal, calm.** **Not** futuristic: no neon colours, no glow, no glassmorphism/blur, no gradients, no sci-fi fonts.
- It should feel like Linear, Notion or Apple Music: lots of white space and clear typography.
- **Font:** Inter (`@fontsource-variable/inter`, bundled locally, no CDN). Numbers are always tabular (`font-variant-numeric: tabular-nums`, class `.num`).
- **Font sizes:** only 12 / 14 / 16 / 20 / 28 px (`text-xs` … `text-xl`).
- **Colours:** neutral greys and **exactly one** calm accent colour (muted blue `#3B6FD8`). Light and dark mode are always both supported.
- **Spacing** on a 4px grid. **Radius** 8px, cards 12px.
- **Separation** through subtle 1px borders (`border-border`), **no shadows**.
- **Icons:** `lucide-react`, 18px, stroke width 1.75.
- **Animations** only subtle: 150–200ms ease-out. Always respect `prefers-reduced-motion`.
- All raw values live in `apps/desktop/src/styles/tokens.css` (`--cove-*`, light/dark via `data-theme`, accent via `data-accent`). `styles/theme.css` maps them to Tailwind utilities (`bg-surface`, `text-fg-muted`, `border-border`, `bg-accent` …).
- In `theme.css`, Tailwind's default palette, shadows, blur and every font size except the five allowed ones are **switched off**. Classes like `bg-pink-500`, `shadow-lg` or `text-4xl` generate no CSS. Do not switch them back on. If a token is missing, add it to `tokens.css` and `theme.css`.
- **No hard-coded colours in components.** ESLint forbids hex, `rgb()` and `hsl()` literals in `apps/desktop/src`.
- Icon size and stroke width are set globally by `LucideProvider` in `App.tsx`. Don't pass `size` or `strokeWidth` to icons.
- Always use `GameCover` for game artwork. Without an image URL it renders a calm, flat placeholder.
- Every page needs a skeleton loading state (no spinners), an empty state with friendly text (`EmptyState`) and an error state with a retry action.
- Focus states are always visible (`:focus-visible`). Everything must be usable with the keyboard.
- The UI copy is currently German.

## Code rules

- **TypeScript:** `strict` plus `noUncheckedIndexedAccess` and `exactOptionalPropertyTypes`. **No `any`**: use `unknown` and type guards. ESLint (`typescript-eslint` strictTypeChecked) must pass without errors.
- **JavaScript (plugins):** ES modules, JSDoc types, ESLint recommended.
- **Rust:** `cargo clippy --all-targets -- -D warnings` (pedantic enabled) without warnings, `cargo fmt`. No `unsafe`. No `unwrap()` outside tests; handle errors as `Result`.
- **C#:** `Nullable` enabled, `TreatWarningsAsErrors`. Everything must be AOT compatible, so no reflection-based serialization: JSON only via the source-generated `RpcJsonContext`. `dotnet format`.
- **Python:** 3.12, `ruff check` + `ruff format`, `mypy --strict`. No `Any` in signatures (ruff `ANN401`).
- **Frontend data access:** The UI gets data **only** through `apps/desktop/src/api/*` (`import { api } from "…/api"`). All core calls go through the typed `call()` / `subscribe()` in `api/transport.ts`. Outside `src/api` (and `src/test`), ESLint forbids imports from `src/mocks`, `@tauri-apps/api/core` and `@tauri-apps/api/event`.
- **Mocks:** Only the **catalogue** is still mocked (`src/mocks`). Library, playtime, settings, downloads, plugins and sources come from the Rust core.
- **New Tauri command:** always change all five places together:
  1. function in `src-tauri/src/commands.rs`
  2. entry in the `cove_handlers!` macro in `lib.rs`
  3. signature in `packages/shared/src/commands.ts` (`CoveCommands`, camelCase arguments)
  4. wrapper in `src/api/*`
  5. handler in the test fake `src/test/fakeCore.ts`

  Also cover the command in `src-tauri/src/ipc_tests.rs` with the real JSON payload.
- **Errors** from Rust are `AppError` (thiserror) and reach the frontend as `{ kind, message }`, there as `CoveError`. The `message` is user-facing text (currently German). A new `kind` also goes into `AppErrorKind` in `models.ts`.
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

App data (SQLite `cove.db`, Proton prefixes under `compat/`, plugins under `plugins/`) lives in Tauri's app data folder, on Linux `~/.local/share/app.cove.desktop/`. In the browser (`npm run dev -w @cove/desktop`) only the catalogue works; everything from the core shows "only available in the desktop app".

### Connectors: C# (`services/connectors`)
```sh
dotnet build
dotnet test
dotnet format --verify-no-changes
dotnet run --project src/Cove.Connectors
dotnet publish src/Cove.Connectors -c Release -r linux-x64   # Native AOT (win-x64 on Windows)
```

### Recommender: Python (`services/recommender`)
```sh
uv sync
uv run cove-recommender
uv run ruff check . && uv run ruff format --check .
uv run mypy
uv run pytest
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
