/**
 * Test-only stand-in for the Rust core: answers typed commands in memory via
 * Tauri's `mockIPC`. Keeps the UI tests independent of a running app.
 */
import type {
  AppErrorKind,
  AppSettings,
  CoveEvents,
  CoveCommand,
  CoveCommands,
  Game,
  LibraryItem,
  ScanCandidate,
} from "@cove/shared";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

/** Rejects like the Rust core: an error carrying `kind` + `message`. */
function reject(kind: AppErrorKind, message: string): never {
  throw Object.assign(new Error(message), { kind });
}

type Handlers = { [C in CoveCommand]: (args: CoveCommands[C]["args"]) => CoveCommands[C]["result"] };

export function fakeGame(id: string, title: string): Game {
  return {
    id,
    title,
    summary: "Kurz.",
    description: "Lang.",
    genres: ["puzzle"],
    platforms: ["linux"],
    releaseDate: "2024-05-01",
    developer: "Studio",
    publisher: "Verlag",
    rating: 80,
    popularity: 50,
    artwork: { cover: { url: null, alt: "" }, header: { url: null, alt: "" }, screenshots: [] },
  };
}

export function fakeItem(id: string, title: string, overrides: Partial<LibraryItem["entry"]> = {}): LibraryItem {
  return {
    game: fakeGame(id, title),
    entry: {
      gameId: id,
      source: "manual",
      storeGameId: null,
      installed: true,
      installPath: `/games/${id}`,
      executablePath: `/games/${id}/${id}`,
      installSizeBytes: 1_000_000,
      favorite: false,
      playtimeMinutes: 90,
      lastPlayedAt: "2026-11-01T10:00:00Z",
      addedAt: "2026-10-01T10:00:00Z",
      achievements: { unlocked: 0, total: 0 },
      launchOptions: { arguments: "", compatibilityTool: "native", env: [] },
      ...overrides,
    },
  };
}

export const FAKE_SCAN: ScanCandidate[] = [
  { title: "Hollow Lantern", executablePath: "/games/hl/HollowLantern.exe", sizeBytes: 3_000_000, kind: "windows-exe", alreadyInLibrary: false },
  { title: "Tidewater", executablePath: "/games/tide/tide", sizeBytes: 9_000_000, kind: "linux-binary", alreadyInLibrary: true },
];

export const DEFAULT_SETTINGS: AppSettings = {
  general: { language: "de", launchOnStartup: false, minimizeToTray: false, startPage: "home" },
  appearance: { theme: "system", accent: "blue" },
  library: { scanFolders: [], scanOnStartup: false },
  downloads: { directory: "/home/u/Games", bandwidthLimitBytesPerSecond: null, maxConcurrent: 1 },
};

export interface FakeCore {
  library: LibraryItem[];
  settings: AppSettings;
  calls: { command: string; args: unknown }[];
}

/** Installs the fake core. Returns its (mutable) state for assertions. */
export function installFakeCore(library: LibraryItem[] = [fakeItem("tidewater", "Tidewater")]): FakeCore {
  const core: FakeCore = { library: structuredClone(library), settings: structuredClone(DEFAULT_SETTINGS), calls: [] };
  const find = (gameId: string) => core.library.find((i) => i.game.id === gameId) ?? null;
  const require = (gameId: string) => {
    const item = find(gameId);
    if (!item) reject("notInLibrary", `„${gameId}“ ist nicht in deiner Bibliothek.`);
    return item;
  };

  const handlers: Handlers = {
    library_list: () => core.library,
    library_get: ({ gameId }) => find(gameId),
    library_add_manual: ({ input }) => {
      if (input.executablePath.includes("missing")) {
        reject("invalidInput", `Datei nicht gefunden: ${input.executablePath}`);
      }
      const id = `manual-${input.title.toLowerCase().replace(/\W+/g, "-")}`;
      const item = fakeItem(id, input.title, { executablePath: input.executablePath, playtimeMinutes: 0, lastPlayedAt: null });
      core.library.push(item);
      return item;
    },
    library_add_from_catalog: ({ game }) => {
      const item = fakeItem(game.id, game.title, { installed: false, executablePath: null, installPath: null });
      core.library.push(item);
      return item;
    },
    library_remove: ({ gameId }) => {
      require(gameId);
      core.library = core.library.filter((i) => i.game.id !== gameId);
      return null;
    },
    library_update: ({ gameId, patch }) => {
      const item = require(gameId);
      if (patch.favorite !== undefined) item.entry.favorite = patch.favorite;
      if (patch.launchOptions) item.entry.launchOptions = patch.launchOptions;
      if (patch.executablePath) {
        item.entry.executablePath = patch.executablePath;
        item.entry.installed = true;
      }
      if (patch.title) item.game.title = patch.title;
      return item;
    },
    library_scan_folder: ({ path }) => {
      if (path === "/nope") reject("invalidInput", "„/nope“ ist kein Ordner.");
      return FAKE_SCAN;
    },
    library_achievements: () => [],
    library_playtime: ({ gameId }) => ({
      gameId,
      totalMinutes: 90,
      last14DaysMinutes: 30,
      sessionCount: 2,
      days: Array.from({ length: 14 }, (_, i) => ({
        date: new Date(Date.UTC(2026, 9, 21 + i)).toISOString().slice(0, 10),
        minutes: i === 13 ? 30 : 0,
      })),
    }),
    achievements_recent: () => [],
    game_launch: ({ gameId }) => ({ gameId, method: "process", tracked: true }),
    game_stop: ({ gameId }) => {
      reject("notRunning", `${gameId} läuft nicht.`);
    },
    game_running_list: () => [],
    settings_get: () => core.settings,
    settings_update: ({ settings }) => {
      core.settings = settings;
      return settings;
    },
    sources_list: () =>
      (["steam", "gog", "epic"] as const).map((store) => ({ store, connected: false, accountName: null, gameCount: 0, lastSyncedAt: null })),
    sources_connect: () => {
      reject("notSupported", "Noch nicht verfügbar: Die Verbindung kommt mit dem Connector-Service.");
    },
    plugins_list: () => [],
    plugins_set_enabled: () => {
      reject("invalidInput", "Plugin nicht gefunden");
    },
    downloads_list: () => [],
    downloads_enqueue: () => null,
    downloads_set_status: () => null,
    downloads_reorder: () => null,
    downloads_clear_history: () => null,
    system_profile: () => ({ id: "local", displayName: "Testperson", avatarUrl: null }),
    system_about: () => ({ appVersion: "0.1.0", platform: "linux", components: [{ name: "core", version: "0.1.0" }] }),
  };

  clearMocks();
  mockIPC(
    (command, payload) => {
      core.calls.push({ command, args: payload });
      if (command.startsWith("plugin:")) return null;
      const handler = handlers[command as CoveCommand] as ((args: unknown) => unknown) | undefined;
      if (!handler) reject("io", `Unbekannter Befehl ${command}`);
      return structuredClone(handler(payload ?? {}));
    },
    { shouldMockEvents: true },
  );
  return core;
}

/** Emits a core event to the UI (requires `installFakeCore`, which enables event mocking). */
export async function emitCoreEvent<E extends keyof CoveEvents>(event: E, payload: CoveEvents[E]): Promise<void> {
  const { emit } = await import("@tauri-apps/api/event");
  await emit(event, payload);
}
