# Cove

A calm, universal desktop game launcher for **Linux and Windows** (macOS later).
Cove brings games from Steam, GOG, Epic and manually added titles together in **one** library,
launches them (on Linux optionally through Proton or Wine) and tracks your playtime.

> Status: early development. Library, folder scan, game launching and playtime tracking work.
> Store accounts and downloads arrive with the connector service.

## Features

- **Library** in grid or list view, grouped by installed, favourites and source
- **Folder scan:** finds `.exe` files, Linux binaries, AppImages, `.desktop` entries and launch scripts.
  Installers, uninstallers and crash reporters are ignored. You confirm what gets added.
- **Game launching:** native, via Proton or Wine (Linux), or through store URL schemes (`steam://rungameid/…`),
  with custom launch arguments and environment variables
- **Playtime tracking:** follows the whole process tree, stores sessions and shows a 14-day chart
- **Command palette** (`Ctrl+K`), light and dark mode, five calm accent colours, fully keyboard accessible
- **Plugins** in plain JavaScript (ES modules)

## Architecture

| Area | Technology | Folder |
|---|---|---|
| Core | Rust, Tauri 2, SQLite (sqlx) | `apps/desktop/src-tauri` |
| UI | React 19, TypeScript, Vite, Tailwind CSS v4, Zustand | `apps/desktop/src` |
| Store connectors & achievements | C# .NET 9 (Native AOT), JSON-RPC over stdio | `services/connectors` |
| Recommendations | Python 3.12 (uv), JSON-RPC over stdio | `services/recommender` |
| Plugins | JavaScript (ES modules) | `plugins` |
| Catalogue proxy (IGDB, no user keys) | TypeScript, Cloudflare Workers, Hono | `services/catalog-proxy` |
| Shared types & JSON schemas | TypeScript | `packages/shared` |

Details in [docs/architecture.md](docs/architecture.md). Design and code rules live in [CLAUDE.md](CLAUDE.md).

## Requirements

- Node.js 22+
- Rust (stable) with `clippy` and `rustfmt`
- Linux: Tauri system packages. On Fedora:
  `sudo dnf install webkit2gtk4.1-devel gtk3-devel libsoup3-devel`
  ([other distributions](https://v2.tauri.app/start/prerequisites/))
- Optional, for the services:
  - [.NET SDK 9](https://dotnet.microsoft.com/download) (Native AOT also needs `clang` and `zlib-devel`)
  - [uv](https://docs.astral.sh/uv/)

## Getting started

```sh
npm install
npm run dev          # starts the desktop app (Tauri + Vite)
```

App data such as the SQLite database and Proton prefixes lives in the app data folder,
on Linux `~/.local/share/app.cove.desktop/`.

## Checks

```sh
npm run check                                          # ESLint, TypeScript, Vitest, plugin tests
(cd apps/desktop/src-tauri && cargo clippy --all-targets -- -D warnings && cargo test)
(cd services/connectors && dotnet test)
(cd services/recommender && uv run ruff check . && uv run mypy && uv run pytest)
```

Game data provided by [IGDB.com](https://www.igdb.com). To run your own catalogue proxy, see
[docs/proxy.md](docs/proxy.md).

## License

[MIT](LICENSE)
