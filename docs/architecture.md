# Cove — Architektur

## Prozesse

```
┌──────────────────────────── Cove Desktop (Tauri 2) ────────────────────────────┐
│  UI (React + TS, WebView)  ──invoke()──▶  Core (Rust, src-tauri)               │
│                                             │  spawnt & überwacht Sidecars     │
└─────────────────────────────────────────────┼──────────────────────────────────┘
                     stdin/stdout (NDJSON)    │    stdin/stdout (NDJSON)
              ┌───────────────────────────────┴───────────────────────┐
              ▼                                                       ▼
   cove-connectors (C# .NET 9, Native AOT)              cove-recommender (Python 3.12)
   Store-Connectors, Achievements                       Empfehlungen
```

- **Core (Rust)** ist der einzige Prozess, mit dem die UI spricht. Er besitzt die
  Bibliothek, startet Spiele, lädt Plugins und startet/überwacht die Sidecars.
- **UI (TypeScript/React)** ruft ausschließlich Tauri-Commands auf, nie direkt Sidecars.
- **Plugins (JavaScript, ES Modules)** laufen ohne Build-Schritt; API folgt später.
- **Connectors (C#)** und **Recommender (Python)** sind zustandsarme Worker-Prozesse.

## RPC-Protokoll

- JSON-RPC 2.0, **eine Nachricht pro Zeile** (newline-delimited JSON, UTF-8).
- Core → Service über `stdin`, Service → Core über `stdout`.
- `stderr` ist ausschließlich für Logs.
- Jede Methode hat:
  - TS-Typen in `packages/shared/src/methods.ts`
  - ein JSON-Schema in `packages/shared/schemas/<method>.schema.json`
  - gespiegelte Typen in Rust (`commands.rs`), C# (`Rpc/Messages.cs`), Python (`rpc.py`).
- Methodennamen: `<bereich>.<verb>` in camelCase, z. B. `system.ping`, `library.list`.
- Jeder Service implementiert `system.ping` → `{ service, version, message }`.

## Frontend (`apps/desktop/src`)

```
api/         Datenzugriffsschicht – heute Mocks, später Tauri invoke(); einziger Importeur von mocks/
mocks/       In-Memory-Mock-Backend (~40 erfundene Spiele, Download-Simulation)
stores/      Zustand: Präferenzen (persistiert), UI, Downloads, Katalogfilter, Toasts
hooks/       useQuery, useInfiniteCatalog, useHotkey, useHistoryNav
components/  ui/ (Primitives), layout/ (Shell, Sidebar, Topbar/Titelbar, Command-Palette), game/
pages/       Home, Catalogue (+Detail), Library (+Detail), Downloads, Settings
styles/      tokens.css (Rohwerte) + theme.css (Tailwind v4 Theme)
```

- Routing: `createHashRouter`, funktioniert im Browser und unter Tauri gleich.
- Eigene Titelbar: `decorations: false`. Die Topbar ist Drag-Region (`data-tauri-drag-region`), Fensterbuttons in `WindowControls.tsx`.
- Tastatur: `Ctrl+K` öffnet die Command-Palette, `Ctrl+B` klappt die Sidebar ein und aus, `Alt+←/→` navigiert zurück und vor.

## Core (`apps/desktop/src-tauri/src`)

```
lib.rs            Setup (DB öffnen, State, Plugins), Command-Liste (cove_handlers!)
commands.rs       Tauri-Commands (dünn: validieren → db/scanner/launcher)
error.rs          AppError (thiserror) → { kind, message }
models.rs         Wire-Typen, spiegeln packages/shared/src/models.ts
db/               sqlx-Repositories: library, sessions, achievements, settings, downloads, sources
scanner.rs        Ordner-Scan mit Heuristik (Vorschläge, der User bestätigt)
launcher.rs       Startplan, Prozessbaum-Tracking, Sessions, Events
plugins.rs        Plugin-Erkennung (manifest.json) + Aktiv-Status
migrations/       SQL-Migrationen (eingebettet über sqlx::migrate!)
```

### Spiel starten

1. **Steam-Spiele** starten immer über `steam://rungameid/<id>` (DRM, Overlay, Cloud-Saves).
2. Spiele mit **Startdatei** startet Cove direkt:
   - Linux-Binaries, AppImages und Skripte nativ.
   - `.desktop`-Dateien über ihre `Exec`-Zeile.
   - `.exe` unter Linux über Wine (`wine <exe>`) oder Proton (`<proton>/proton run <exe>`, Prefix pro Spiel unter `compat/<gameId>`). Proton wird in der Steam-Library und in `compatibilitytools.d` gesucht.
3. **GOG und Epic ohne Startdatei** laufen über die URL-Schemes `goggalaxy://openGameView/<id>` und `com.epicgames.launcher://apps/<id>?action=launch&silent=true`.

Startparameter werden shell-artig gesplittet (shlex), Umgebungsvariablen überschreiben Cove-eigene.

### Spielzeit-Tracking

- Spiele starten in einer eigenen Prozessgruppe. Der Tracker übernimmt alle Nachfahren des Startprozesses und alle Prozesse der Gruppe (Linux: `/proc/<pid>/stat`). Dadurch zählen auch Kinder von Launchern, die sich sofort beenden.
- Gegen wiederverwendete PIDs schützt der Vergleich der Startzeit.
- Bei URL-Starts mit bekanntem Installationsordner übernimmt Cove jeden Prozess, dessen Executable darin liegt. Ohne Ordner kann Cove nicht messen (`LaunchResult.tracked = false`).
- Eine Session steht in `play_sessions` mit einem Heartbeat alle 30 s. Nach einem Absturz gehen höchstens 30 s verloren, offene Sessions werden beim nächsten Start geschlossen.
- Events: `game-state-changed` (`running` / `stopped` mit Sitzungslänge) und `downloads-changed`.

### Ordner-Scan (Heuristik)

1. Walk bis Tiefe 4. Versteckte Ordner, Redistributables und `Engine/` werden übersprungen.
2. Kandidaten sind `.exe`, ELF-Binaries und `.sh` mit Ausführungsrecht, `.AppImage` und `.desktop` (`Type=Application`, nicht versteckt).
3. Namen wie unins*, setup, install*, crash*, redist, updater, helper, EasyAntiCheat oder BattlEye fliegen raus.
4. Jeder Unterordner gilt als ein Spiel. AppImages und `.desktop`-Dateien direkt im Ordner sind eigene Spiele. Gibt es Ordner wie `bin/` oder `x86_64/`, gilt der gescannte Ordner selbst als ein Spiel.
5. Pro Spiel gewinnt der Kandidat mit dem besten Score:
   - Namensähnlichkeit zum Ordner
   - geringe Tiefe
   - Dateigröße
   - 64-Bit-Pfad
   - unter Linux: nativ vor `.exe`
   - „launcher“ im Namen gibt Abzug

## Offene Punkte

- Sidecar-Packaging (Tauri `externalBin`) und Lifecycle im Core
- Echter Downloader (Connectors). Die Queue in `downloads` ist da, überträgt aber noch nichts.
- Spielzeit-Tracking unter Windows über Job Objects statt Polling
- Plugin-API und Sandboxing
- Persistenz (SQLite im Core?)
