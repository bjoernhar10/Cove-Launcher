# Cove — Hinweise für Claude

Cove ist ein universeller Desktop-Game-Launcher für Linux und Windows (macOS später).
Er führt Spiele aus allen Stores und lokale Spiele in einer ruhigen, aufgeräumten Bibliothek zusammen.
Architektur-Details: `docs/architecture.md`.

## Sprachen und Rollen

| Bereich | Sprache | Ort | Rolle |
|---|---|---|---|
| Core | **Rust** (Tauri 2) | `apps/desktop/src-tauri` | Bibliothek, Spielstart, Prozess-/Sidecar-Verwaltung, Plugin-Host, einzige Brücke zur UI |
| UI | **TypeScript** + React + Vite | `apps/desktop/src` | Oberfläche; spricht nur über Tauri-Commands mit dem Core |
| Plugins | **JavaScript** (ES Modules, kein Build) | `plugins/` | Erweiterungen durch Dritte |
| Store-Connectors & Achievements | **C#** .NET 9, Native AOT | `services/connectors` | Steam/GOG/Epic/... anbinden, Achievements |
| Empfehlungen | **Python** 3.12 (uv) | `services/recommender` | Empfehlungs-Engine |
| Gemeinsame Verträge | TypeScript + JSON-Schema | `packages/shared` | Typen und Schemas für **alle** RPC-Nachrichten |

**Kommunikation:** JSON-RPC 2.0 über stdio, eine JSON-Nachricht pro Zeile. stdout ist nur für RPC, Logs gehen nach stderr.
Wenn du eine neue RPC-Methode anlegst oder änderst, aktualisierst du **im selben Schritt**:
TS-Typen (`packages/shared/src/methods.ts`), das JSON-Schema (`packages/shared/schemas/`) und die gespiegelten Typen in Rust, C# und Python.

## Design-Regeln (verbindlich für jede UI-Änderung)

- **Clean, minimalistisch, ruhig.** Bitte **nicht** futuristisch: keine Neon-Farben, kein Glow, keine Glassmorphism-/Blur-Effekte, keine Gradients, keine Sci-Fi-Fonts.
- Das Gefühl soll an Linear, Notion oder Apple Music erinnern: viel Weißraum und klare Typografie.
- **Font:** Inter (`@fontsource-variable/inter`, lokal gebündelt, kein CDN). Zahlen immer tabular (`font-variant-numeric: tabular-nums`, Klasse `.num`).
- **Schriftgrößen:** nur 12 / 14 / 16 / 20 / 28 px (`--text-xs` … `--text-xl`).
- **Farben:** neutrale Graustufen und **genau eine** ruhige Akzentfarbe (gedecktes Blau `#3B6FD8`). Light- und Dark-Mode werden immer beide unterstützt.
- **Spacing** im 4px-Raster (`--space-*`). **Radius** 8px, für Karten 12px.
- **Abgrenzung** über dezente 1px-Borders (`--color-border`), **keine Schatten**.
- **Icons:** `lucide-react`, 18px, `strokeWidth={1.75}`.
- **Animationen** nur subtil: 150–200ms ease-out (`--duration-fast`/`--duration-normal`, `--ease-out`). `prefers-reduced-motion` wird immer respektiert.
- Alle Rohwerte stehen in `apps/desktop/src/styles/tokens.css` (`--cove-*`, Light/Dark über `data-theme`, Akzent über `data-accent`). `styles/theme.css` bildet sie auf Tailwind-Utilities ab (`bg-surface`, `text-fg-muted`, `border-border`, `bg-accent` …).
- In `theme.css` sind Tailwinds Standardpalette, Schatten, Blur und alle Schriftgrößen außer den fünf erlaubten **abgeschaltet**. Klassen wie `bg-pink-500`, `shadow-lg` oder `text-4xl` erzeugen also kein CSS. Bitte nicht wieder einschalten. Fehlt ein Token, ergänzt du es in `tokens.css` und `theme.css`.
- **Keine hartkodierten Farben in Komponenten.** ESLint verbietet Hex-, `rgb()`- und `hsl()`-Literale in `apps/desktop/src`.
- Icon-Größe und Strichstärke setzt `LucideProvider` in `App.tsx` global. Gib Icons deshalb kein `size` oder `strokeWidth` mit.
- Für Spiel-Artwork nutzt du immer `GameCover`. Ohne Bild-URL rendert es einen ruhigen, flachen Platzhalter.
- Jede Seite braucht einen Skeleton-Ladezustand (kein Spinner), einen Leerzustand mit freundlichem Text (`EmptyState`) und einen Fehlerzustand mit „Erneut versuchen“.
- Fokus-Zustände sind immer sichtbar (`:focus-visible`). Die Bedienung per Tastatur muss möglich sein.

## Code-Regeln

- **TypeScript:** `strict` plus `noUncheckedIndexedAccess` und `exactOptionalPropertyTypes`. **Kein `any`**: lieber `unknown` und Typ-Guards. ESLint (`typescript-eslint` strictTypeChecked) muss ohne Fehler durchlaufen.
- **JavaScript (Plugins):** ES Modules, JSDoc-Typen, ESLint recommended.
- **Rust:** `cargo clippy --all-targets -- -D warnings` (pedantic ist aktiviert) ohne Warnings, `cargo fmt`. Kein `unsafe`. Kein `unwrap()` außerhalb von Tests, Fehler werden als `Result` behandelt.
- **C#:** `Nullable` enable, `TreatWarningsAsErrors`. Alles muss AOT-kompatibel sein, also keine Reflection-Serialisierung: JSON nur über den source-generierten `RpcJsonContext`. `dotnet format`.
- **Python:** 3.12, `ruff check` + `ruff format`, `mypy --strict`. Kein `Any` in Signaturen (ruff `ANN401`).
- **Datenzugriff im Frontend:** Die UI holt Daten **nur** über `apps/desktop/src/api/*` (`import { api } from "…/api"`). Dort laufen alle Core-Aufrufe über das typisierte `call()` bzw. `subscribe()` in `api/transport.ts`. ESLint verbietet außerhalb von `src/api` (und `src/test`) Importe aus `src/mocks`, `@tauri-apps/api/core` und `@tauri-apps/api/event`.
- **Mocks:** Nur noch der **Katalog** ist gemockt (`src/mocks`). Bibliothek, Spielzeit, Einstellungen, Downloads, Plugins und Quellen kommen aus dem Rust-Core.
- **Neuer Tauri-Command:** Du änderst immer alle fünf Stellen gemeinsam:
  1. Funktion in `src-tauri/src/commands.rs`
  2. Eintrag im Makro `cove_handlers!` in `lib.rs`
  3. Signatur in `packages/shared/src/commands.ts` (`CoveCommands`, Argumente camelCase)
  4. Wrapper in `src/api/*`
  5. Handler im Test-Fake `src/test/fakeCore.ts`

  Zusätzlich deckst du den Command in `src-tauri/src/ipc_tests.rs` mit dem echten JSON-Payload ab.
- **Fehler** aus Rust sind `AppError` (thiserror) und kommen als `{ kind, message }` im Frontend an, dort als `CoveError`. Die `message` ist deutscher Klartext für die UI. Ein neuer `kind` gehört auch in `AppErrorKind` in `models.ts`.
- **Rust-Typen** in `src-tauri/src/models.rs` spiegeln `packages/shared/src/models.ts`. `contract_tests.rs` prüft serialisierte Werte gegen `models.schema.json`.
- **Datenbank:** SQLite über sqlx mit Runtime-Queries (kein `query!`-Makro, also kein `DATABASE_URL` nötig). Schemaänderungen kommen immer als **neue** Datei `src-tauri/migrations/NNNN_*.sql`; bestehende Migrationen bearbeitest du nie. DB-Funktionen bekommen Tests mit `db::open_in_memory()`.
- **Datentypen** (Game, LibraryEntry, Achievement, Download, CatalogFilter …) liegen in `packages/shared/src/models.ts`. Nach jeder Änderung erzeugst du das Schema mit `npm run schemas -w @cove/shared` neu. Ein Test prüft, dass es aktuell ist, ein weiterer, dass die Mock-Daten zum Schema passen.
- **State:** Zustand-Stores liegen in `apps/desktop/src/stores`, Server-Daten lädst du mit `useQuery` (`hooks/useQuery.ts`). Nach Mutationen rufst du `invalidateQueries()` auf.
- Tests liegen neben dem Code (TS/JS: `*.test.ts(x)`/`*.test.js`; Rust: `#[cfg(test)]`) bzw. in `tests/` (C#, Python).
- Abhängigkeiten installierst du über den Paketmanager (`npm i`, `cargo add`, `uv add`, `dotnet add package`). Versionen werden nicht geraten.
- TypeScript bleibt auf `~6.0` gepinnt, bis `typescript-eslint` TS 7 unterstützt.

## Befehle

Voraussetzungen: Node 22+, Rust (stable, mit `clippy` und `rustfmt`), .NET SDK 9, `uv`. Unter Linux kommen die Tauri-Systempakete hinzu (webkit2gtk-4.1, gtk3, libsoup3). Für AOT-Publish braucht es außerdem `clang` und `zlib-devel`.

### Root (TS/JS, alle Workspaces)
```sh
npm install              # einmalig
npm run dev              # startet die Desktop-App (Tauri + Vite)
npm run lint             # ESLint + tsc für alle Workspaces
npm run test             # Vitest (shared, desktop) + node:test (plugins)
npm run check            # lint + test
npm run schemas -w @cove/shared   # JSON-Schema aus models.ts neu erzeugen
```

### Desktop: UI + Rust-Core (`apps/desktop`)
```sh
npm run tauri:dev -w @cove/desktop     # App mit Hot-Reload
npm run dev -w @cove/desktop           # nur Frontend im Browser (http://localhost:1420), Fensterbuttons ohne Funktion
npm run tauri:build -w @cove/desktop   # Release-Bundle
cd apps/desktop/src-tauri
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo test
```

Daten der App (SQLite `cove.db`, Proton-Prefixe unter `compat/`, Plugins unter `plugins/`) liegen in Tauris App-Data-Ordner, unter Linux `~/.local/share/app.cove.desktop/`. Im Browser (`npm run dev -w @cove/desktop`) läuft nur der Katalog; alle Core-Daten zeigen „Das geht nur in der Desktop-App“.

### Connectors: C# (`services/connectors`)
```sh
dotnet build
dotnet test
dotnet format --verify-no-changes
dotnet run --project src/Cove.Connectors
dotnet publish src/Cove.Connectors -c Release -r linux-x64   # Native AOT (win-x64 unter Windows)
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

Smoke-Test für einen Service:
`echo '{"jsonrpc":"2.0","id":1,"method":"system.ping","params":{"message":"hi"}}' | <service-befehl>`

## Arbeitsregel

**Nach jeder Änderung führst du Lint und Tests für alle betroffenen Teilprojekte aus, bevor du „fertig“ meldest.**
Schlägt etwas fehl, behebst du es oder nennst den Fehler ausdrücklich. Ein Werkzeug fehlt lokal? Dann sagst du das, statt einen Erfolg zu behaupten.
