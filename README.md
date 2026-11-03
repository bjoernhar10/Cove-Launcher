# Cove

Ein ruhiger, universeller Desktop-Game-Launcher für **Linux und Windows** (macOS folgt).
Cove bringt Spiele aus Steam, GOG, Epic und manuell hinzugefügte Spiele in **einer** Bibliothek zusammen,
startet sie – unter Linux auf Wunsch über Proton oder Wine – und erfasst die Spielzeit.

> Status: frühe Entwicklung. Bibliothek, Ordner-Scan, Spielstart und Spielzeit-Tracking funktionieren.
> Der Katalog nutzt noch Beispieldaten, Store-Konten und Downloads kommen mit dem Connector-Service.

## Funktionen

- **Bibliothek** in Raster- oder Listenansicht, gruppiert nach Installiert, Favoriten und Quelle
- **Ordner-Scan:** findet `.exe`, Linux-Binaries, AppImages, `.desktop`-Dateien und Startskripte.
  Installer, Uninstaller und Crash-Reporter werden ignoriert. Du bestätigst, was hinzukommt.
- **Spiel starten:** nativ, über Proton oder Wine (Linux) oder über Store-URL-Schemes (`steam://rungameid/…`),
  mit eigenen Startparametern und Umgebungsvariablen
- **Spielzeit-Tracking:** verfolgt den ganzen Prozessbaum, speichert Sitzungen und zeigt eine 14-Tage-Statistik
- **Command-Palette** (`Ctrl+K`), Light und Dark Mode, fünf ruhige Akzentfarben, vollständig per Tastatur bedienbar
- **Plugins** in reinem JavaScript (ES Modules)

## Aufbau

| Bereich | Technik | Ordner |
|---|---|---|
| Core | Rust, Tauri 2, SQLite (sqlx) | `apps/desktop/src-tauri` |
| Oberfläche | React 19, TypeScript, Vite, Tailwind CSS v4, Zustand | `apps/desktop/src` |
| Store-Connectors & Achievements | C# .NET 9 (Native AOT), JSON-RPC über stdio | `services/connectors` |
| Empfehlungen | Python 3.12 (uv), JSON-RPC über stdio | `services/recommender` |
| Plugins | JavaScript (ES Modules) | `plugins` |
| Gemeinsame Typen & JSON-Schemas | TypeScript | `packages/shared` |

Mehr dazu in [docs/architecture.md](docs/architecture.md). Die verbindlichen Design- und Code-Regeln stehen in [CLAUDE.md](CLAUDE.md).

## Voraussetzungen

- Node.js 22+
- Rust (stable) mit `clippy` und `rustfmt`
- Linux: Tauri-Systempakete. Unter Fedora:
  `sudo dnf install webkit2gtk4.1-devel gtk3-devel libsoup3-devel`
  ([andere Distributionen](https://v2.tauri.app/start/prerequisites/))
- Optional für die Services:
  - [.NET SDK 9](https://dotnet.microsoft.com/download) (Native AOT braucht zusätzlich `clang` und `zlib-devel`)
  - [uv](https://docs.astral.sh/uv/)

## Loslegen

```sh
npm install
npm run dev          # startet die Desktop-App (Tauri + Vite)
```

Daten wie die SQLite-Datenbank und Proton-Prefixe liegen im App-Data-Ordner,
unter Linux `~/.local/share/app.cove.desktop/`.

## Prüfen

```sh
npm run check                                          # ESLint, TypeScript, Vitest, Plugin-Tests
(cd apps/desktop/src-tauri && cargo clippy --all-targets -- -D warnings && cargo test)
(cd services/connectors && dotnet test)
(cd services/recommender && uv run ruff check . && uv run mypy && uv run pytest)
```

## Lizenz

[MIT](LICENSE)
