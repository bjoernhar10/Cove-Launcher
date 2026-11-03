# Cove — Architecture

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
api/         Data access layer: typed Tauri invoke()/listen(); the only importer of mocks/
mocks/       Catalogue mock (~40 fictional games) — the only remaining mock
stores/      Zustand: preferences (persisted), UI, downloads, running games, catalogue filter, toasts
hooks/       useQuery, useInfiniteCatalog, useHotkey, useHistoryNav
components/  ui/ (primitives), layout/ (shell, sidebar, topbar/title bar, command palette), game/
pages/       Home, Catalogue (+detail), Library (+detail), Downloads, Settings
styles/      tokens.css (raw values) + theme.css (Tailwind v4 theme)
test/        Fake core for UI tests (Tauri mockIPC)
```

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

## Open points

- Sidecar packaging (Tauri `externalBin`) and lifecycle in the core
- Real downloader (connectors). The queue in `downloads` exists but transfers nothing yet.
- Playtime tracking on Windows via Job Objects instead of polling
- Plugin API and sandboxing
