/**
 * Test-only stand-in for the Rust core: answers typed commands in memory via
 * Tauri's `mockIPC`. Keeps the UI tests independent of a running app.
 */
import type {
  AppErrorKind,
  AppSettings,
  CatalogPage,
  ConnectableStore,
  CoveAchievement,
  LocalStoreGame,
  StoreAccount,
  CoveEvents,
  MatchCandidate,
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
    features: null,
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
      catalogId: null,
      matchStatus: "none",
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
  catalog: { useCoveService: true },
};

/** Catalogue served by the fake core (provider "igdb"). */
export const FAKE_CATALOG: Game[] = [
  { ...fakeGame("igdb-1", "Tidewater"), rating: 88 },
  { ...fakeGame("igdb-2", "Hollow Lantern"), genres: ["horror"] },
  { ...fakeGame("igdb-3", "Paper Orbit"), releaseDate: null },
];

export function fakeAccount(store: ConnectableStore, overrides: Partial<StoreAccount> = {}): StoreAccount {
  return {
    store,
    status: "disconnected",
    connected: false,
    accountName: null,
    gameCount: 0,
    lastSyncedAt: null,
    loginMode: store === "steam" ? "browser" : "webview",
    problem: null,
    ...overrides,
  };
}

export const FAKE_LOCAL: LocalStoreGame[] = [
  { store: "steam", storeGameId: "105600", title: "Terraria", installPath: "/s/Terraria", executablePath: null, sizeBytes: 470_000_000, inLibrary: false },
  { store: "gog", storeGameId: "1207664663", title: "The Witcher 3", installPath: "/g/Witcher3", executablePath: null, sizeBytes: null, inLibrary: true },
];

export const FAKE_COVE_ACHIEVEMENTS: CoveAchievement[] = [
  { id: "first-hour", title: "Erste Stunde", description: "Spiele insgesamt eine Stunde über Cove.", unlockedAt: "2026-11-01T10:00:00Z", current: 1, target: 1 },
  { id: "ten-hours", title: "10 Stunden", description: "Spiele insgesamt 10 Stunden über Cove.", unlockedAt: null, current: 3, target: 10 },
];

export interface FakeCore {
  library: LibraryItem[];
  settings: AppSettings;
  calls: { command: string; args: unknown }[];
  /** Catalogue state: configured providers and whether the provider is reachable. */
  catalog: { cove: boolean; igdb: boolean; rawg: boolean; offline: boolean };
  /** Whether the recommender sidecar is running. */
  recommender: boolean;
  candidates: Record<string, MatchCandidate[]>;
  accounts: Record<ConnectableStore, StoreAccount>;
  local: LocalStoreGame[];
  coveAchievements: CoveAchievement[];
}

/** Installs the fake core. Returns its (mutable) state for assertions. */
export function installFakeCore(library: LibraryItem[] = [fakeItem("tidewater", "Tidewater")]): FakeCore {
  const core: FakeCore = {
    library: structuredClone(library),
    settings: structuredClone(DEFAULT_SETTINGS),
    calls: [],
    catalog: { cove: true, igdb: false, rawg: false, offline: false },
    candidates: {},
    recommender: true,
    accounts: { steam: fakeAccount("steam"), gog: fakeAccount("gog"), epic: fakeAccount("epic") },
    local: structuredClone(FAKE_LOCAL),
    coveAchievements: structuredClone(FAKE_COVE_ACHIEVEMENTS),
  };
  const requireCatalog = () => {
    const cove = core.catalog.cove && core.settings.catalog.useCoveService;
    if (!cove && !core.catalog.igdb && !core.catalog.rawg) reject("catalogNotConfigured", "Kein Katalog verfügbar.");
  };
  const catalogGame = (id: string) => FAKE_CATALOG.find((g) => g.id === id) ?? reject("catalogProvider", "Eintrag nicht gefunden");
  const page = (items: Game[], p = 0, reasons: Record<string, string> = {}): CatalogPage => ({
    items,
    page: p,
    hasMore: false,
    total: items.length,
    offline: core.catalog.offline,
    reasons,
  });
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
    connectors_list: () => Object.values(core.accounts),
    connector_login_start: ({ store }) => ({
      store,
      mode: core.accounts[store].loginMode,
      fallbackUrl: store === "epic" ? "https://www.epicgames.com/id/login" : null,
    }),
    connector_login_complete: ({ store, code }) => {
      if (code.trim() === "bad") reject("loginFailed", "Das ist kein gültiger Epic-Anmeldecode.");
      core.accounts[store] = { ...core.accounts[store], status: "connected", connected: true, accountName: "Tiny" };
      return core.accounts[store];
    },
    connector_login_cancel: () => null,
    connector_refresh: ({ store }) => {
      if (core.accounts[store].status === "needsReauth") reject("reauthRequired", "Bitte melde dich erneut an.");
      return core.accounts[store];
    },
    connector_logout: ({ store, keepGames }) => {
      core.accounts[store] = fakeAccount(store, { gameCount: keepGames ? core.accounts[store].gameCount : 0 });
      return core.accounts[store];
    },
    connector_detect_local: ({ store }) => core.local.filter((g) => store === null || g.store === store),
    connector_import_library: ({ store }) => {
      const fresh = core.local.filter((g) => g.store === store && !g.inLibrary);
      for (const g of fresh) g.inLibrary = true;
      core.accounts[store].gameCount += fresh.length;
      return { store, added: fresh.length, updated: 0, total: fresh.length };
    },
    achievements_sync: () => ({ games: 1, achievements: 2, newlyUnlocked: 1 }),
    cove_achievements_list: () => core.coveAchievements,
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
    catalog_status: () => {
      const cove = core.catalog.cove && core.settings.catalog.useCoveService;
      return {
        sources: [...(cove ? ["cove" as const] : []), ...(core.catalog.igdb ? ["igdb" as const] : []), ...(core.catalog.rawg ? ["rawg" as const] : [])],
        coveService: { available: core.catalog.cove, enabled: core.settings.catalog.useCoveService },
        configured: { igdb: core.catalog.igdb, rawg: core.catalog.rawg },
        online: !core.catalog.offline,
      };
    },
    catalog_set_credentials: ({ credentials }) => {
      if (credentials.provider === "igdb" && credentials.clientSecret === "wrong") {
        reject("catalogAuth", "Anmeldung beim Katalog fehlgeschlagen: Client-ID oder Secret falsch");
      }
      core.catalog[credentials.provider] = true;
      return handlers.catalog_status({});
    },
    catalog_clear_credentials: ({ provider }) => {
      core.catalog[provider] = false;
      return handlers.catalog_status({});
    },
    catalog_search: ({ query }) => {
      requireCatalog();
      return page(FAKE_CATALOG.filter((g) => g.title.toLowerCase().includes(query.trim().toLowerCase())));
    },
    catalog_browse: ({ filter, sort, page: p }) => {
      requireCatalog();
      const items = FAKE_CATALOG.filter((g) => filter.genres.length === 0 || g.genres.some((x) => filter.genres.includes(x)));
      const reasons: Record<string, string> =
        sort === "recommended" && core.recommender ? Object.fromEntries(items.map((g) => [g.id, `Ähnlich wie ${g.title}`])) : {};
      return page(items, p, reasons);
    },
    catalog_game_details: ({ gameId }) => {
      requireCatalog();
      const linked = core.library.find((i) => i.entry.catalogId === gameId || i.game.id === gameId);
      return { game: catalogGame(gameId), libraryGameId: linked?.game.id ?? null, offline: core.catalog.offline };
    },
    catalog_similar: ({ gameId }) => {
      requireCatalog();
      return page(FAKE_CATALOG.filter((g) => g.id !== gameId));
    },
    library_match_candidates: ({ gameId }) => core.candidates[gameId] ?? [],
    library_link_catalog: ({ gameId, catalogId }) => {
      const item = require(gameId);
      const game = catalogGame(catalogId);
      item.game = { ...game, id: item.game.id };
      item.entry.catalogId = catalogId;
      item.entry.matchStatus = "matched";
      return item;
    },
    library_reject_match: ({ gameId }) => {
      const item = require(gameId);
      item.entry.matchStatus = "rejected";
      return item;
    },
    recommendations_for_user: ({ limit }) => {
      if (!core.recommender) reject("recommenderUnavailable", "Empfehlungen sind gerade nicht verfügbar.");
      requireCatalog();
      return FAKE_CATALOG.slice(0, limit).map((game, i) => ({ game, score: 1 - i / 10, reason: "Weil du 40 Std. Tidewater gespielt hast" }));
    },
    recommendations_discover: ({ limit }) => {
      if (!core.recommender) reject("recommenderUnavailable", "Empfehlungen sind gerade nicht verfügbar.");
      requireCatalog();
      return FAKE_CATALOG.slice(1, 1 + limit).map((game, i) => ({ game, score: 0.5 - i / 20, reason: "Neu für dich: Horror" }));
    },
    recommendations_similar: ({ gameId, limit }) => {
      if (!core.recommender) reject("recommenderUnavailable", "Empfehlungen sind gerade nicht verfügbar.");
      return FAKE_CATALOG.filter((g) => g.id !== gameId).slice(0, limit).map((game) => ({ game, score: 0.5, reason: "Ähnlich" }));
    },
    recommendation_explain: () => {
      if (!core.recommender) reject("recommenderUnavailable", "Empfehlungen sind gerade nicht verfügbar.");
      return "Weil du 40 Std. Tidewater gespielt hast";
    },
    library_rematch: ({ gameId }) => {
      require(gameId);
      return null;
    },
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
