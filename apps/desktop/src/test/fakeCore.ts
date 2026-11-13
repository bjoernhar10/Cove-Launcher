/**
 * Test-only stand-in for the Rust core: answers typed commands in memory via
 * Tauri's `mockIPC`. Keeps the UI tests independent of a running app.
 */
import type {
  AppErrorKind,
  AppSettings,
  CatalogPage,
  Collection,
  ConnectableStore,
  CoveAchievement,
  LocalStoreGame,
  DownloadItem,
  ItchGame,
  PluginInfo,
  StoreAccount,
  CoveEvents,
  MatchCandidate,
  CoveCommand,
  CoveCommands,
  Game,
  DlcOption,
  GameUpdateInfo,
  InstallTarget,
  PrefixAction,
  GameBackups,
  SystemInfo,
  GameTools,
  GameTweaks,
  PrefixInfo,
  ProtonOverview,
  ProtonRating,
  TimeToBeat,
  StoreDlcList,
  LaunchVia,
  LibraryItem,
  ScanCandidate,
  UpdateInfo,
  PlayStats,
  GamepadInfo,
  WishlistItem,
  GamePrice,
  StoreSource,
} from "@cove/shared";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

/** The core's defaults for a game's Linux options. */
export const FAKE_TWEAKS: GameTweaks = {
  mangohud: false,
  gamemode: false,
  gamescope: false,
  gamescopeWidth: null,
  gamescopeHeight: null,
  gamescopeFullscreen: false,
  fsr: false,
  dxvk: true,
  vkd3d: true,
};

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

/** Mirrors `launcher::launch_via` in the Rust core. */
export function launchViaOf(entry: Pick<LibraryItem["entry"], "source" | "storeGameId" | "executablePath">): LaunchVia | null {
  if (entry.source === "steam" && entry.storeGameId !== null) return "steam";
  if (entry.executablePath?.trim()) return "direct";
  if (entry.storeGameId === null) return null;
  return entry.source === "epic" ? "epicLauncher" : entry.source === "gog" ? "gogGalaxy" : null;
}

export function fakeItem(id: string, title: string, overrides: Partial<LibraryItem["entry"]> = {}): LibraryItem {
  const item: LibraryItem = {
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
      launchOptions: { arguments: "", compatibilityTool: "native", env: [], protonVersion: null, tweaks: { ...FAKE_TWEAKS } },
      catalogId: null,
      matchStatus: "none",
      launchVia: null,
      installedByCove: false,
      ...overrides,
    },
  };
  if (!("launchVia" in overrides)) item.entry.launchVia = launchViaOf(item.entry);
  return item;
}

export const FAKE_SCAN: ScanCandidate[] = [
  { title: "Hollow Lantern", executablePath: "/games/hl/HollowLantern.exe", sizeBytes: 3_000_000, kind: "windows-exe", alreadyInLibrary: false },
  { title: "Tidewater", executablePath: "/games/tide/tide", sizeBytes: 9_000_000, kind: "linux-binary", alreadyInLibrary: true },
];

export const DEFAULT_SETTINGS: AppSettings = {
  general: { startInTvMode: false, language: "de", launchOnStartup: false, minimizeToTray: false, startPage: "home" },
  appearance: { theme: "dark", accent: "blue" },
  library: { scanFolders: [], scanOnStartup: false, gameFolders: ["/home/u/Games"], steamSilentStart: true, epicCloudSaves: true, gogCloudSaves: true, defaultProton: "latest", useUmu: true },
  downloads: { directory: "/home/u/Games", bandwidthLimitBytesPerSecond: null, maxConcurrent: 1 },
  catalog: { useCoveService: true },
  backups: { enabled: true, directory: "", keep: 10 },
  emulators: { romFolders: [] },
  wishlist: { country: "DE", priceAlerts: true },
};

/** Systems the fake core offers (a few of the real ones). */
export const FAKE_SYSTEMS: SystemInfo[] = [
  {
    id: "snes",
    name: "Super Nintendo",
    extensions: ["sfc", "smc", "zip"],
    emulators: [{ id: "retroarch", name: "RetroArch", installed: true }],
  },
  {
    id: "ps2",
    name: "PlayStation 2",
    extensions: ["iso", "chd", "cso"],
    emulators: [
      { id: "retroarch", name: "RetroArch", installed: true },
      { id: "pcsx2", name: "PCSX2", installed: false },
    ],
  },
];

/** Catalogue served by the fake core (provider "igdb"). */
export const FAKE_CATALOG: Game[] = [
  { ...fakeGame("igdb-1", "Tidewater"), rating: 88 },
  { ...fakeGame("igdb-2", "Hollow Lantern"), genres: ["horror"] },
  { ...fakeGame("igdb-3", "Paper Orbit"), releaseDate: null },
];

const SOURCE_NAMES: Record<ConnectableStore, string> = { steam: "Steam", gog: "GOG", epic: "Epic Games", amazon: "Amazon" };

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

/** Proton builds the fake core starts with: a newer GE-Proton to download, one installed, one from Steam. */
export function fakeProton(): ProtonOverview {
  const ge = (id: string, installed: boolean, releasedAt: string) => ({
    id,
    name: id,
    kind: "ge" as const,
    installed,
    downloadBytes: 520_000_000,
    releasedAt,
    games: 0,
    removable: false,
  });
  return {
    versions: [
      ge("GE-Proton10-25", false, "2026-11-08T12:00:00Z"),
      ge("GE-Proton10-24", true, "2026-10-20T12:00:00Z"),
      ge("GE-Proton9-27", false, "2025-06-01T12:00:00Z"),
      { id: "steam:Proton 9.0", name: "Proton 9.0", kind: "steam", installed: true, downloadBytes: null, releasedAt: null, games: 0, removable: false },
    ],
    latest: "GE-Proton10-25",
    checkedAt: "2026-11-11T06:00:00Z",
    offline: false,
    installing: [],
    umu: { enabled: true, source: "cove", version: "1.4.4", latest: "1.4.4", python: true },
  };
}

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
  downloads: DownloadItem[];
  itchKey: string | null;
  plugins: PluginInfo[];
  pluginStorage: Record<string, Record<string, unknown>>;
  pluginAchievements: Record<string, unknown[]>;
  /** What `updates_check` reports; `null` = up to date. */
  update: UpdateInfo | null;
  updateInstalled: boolean;
  /** What the native folder picker returns (null = cancelled). */
  pickedFolder: string | null;
  /** Free space per folder (default 500 GB). */
  freeBytes: Record<string, number>;
  /** `game_move` calls. */
  moves: { gameId: string; folder: string }[];
  /** What `game_update_check` reports. */
  gogUpdate: GameUpdateInfo;
  /** Epic games that must stay with the Epic Launcher (reason per game id). */
  launcherOnly: Record<string, string>;
  /** DLCs the install dialog offers (per game id). */
  dlcs: Record<string, DlcOption[]>;
  /** Targets of store installs Cove does itself (per game id). */
  installTargets: Record<string, InstallTarget>;
  /** DLC lists of store games (per game id). */
  gameDlcs: Record<string, StoreDlcList>;
  /** The user's own collections. */
  collections: Collection[];
  /** Proton builds (Linux); `games` and `removable` are computed from the library. */
  proton: ProtonOverview;
  /** Proton builds the user had installed (in order). */
  protonInstalls: string[];
  /** ProtonDB ratings (per game id); games without one have none. */
  protonRatings: Record<string, ProtonRating>;
  /** ProtonDB can't be reached. */
  protonDown: boolean;
  /** Proton prefixes that exist (per game id). */
  prefixes: Record<string, PrefixInfo>;
  /** Prefix actions the user ran, in order. */
  prefixActions: { gameId: string; action: PrefixAction }[];
  /** Save backups per game id; games without an entry are unknown to the manifest. */
  backups: Record<string, GameBackups>;
  /** Systems for the emulator settings. */
  systems: SystemInfo[];
  /** ROM file names per ROM folder path (what a scan finds). */
  romFiles: Record<string, string[]>;
  gamepads: GamepadInfo[];
  tvHint: boolean;
  fullscreen: boolean;
  wishlist: WishlistItem[];
  /** Prices per catalogue id; missing = not found on IsThereAnyDeal. */
  prices: Record<string, GamePrice>;
  /** Linux game tools that are installed. */
  gameTools: GameTools;
  /** Times to beat per catalogue id (IGDB); missing ids have none. */
  timesToBeat: Record<string, TimeToBeat>;
  /** URLs Cove asked the system to open (steam:// …). */
  openedUrls: string[];
  /** Upload ids of extras sent with itch.io downloads. */
  itchExtras: number[][];
}

export function fakePlugin(id: string, overrides: Partial<PluginInfo> = {}): PluginInfo {
  return {
    id,
    name: id,
    version: "1.0.0",
    author: "Test",
    description: "",
    permissions: ["ui.notify"],
    granted: [],
    enabled: false,
    status: "needsApproval",
    error: null,
    removable: false,
    entryUrl: null,
    ...overrides,
  };
}

export const FAKE_ITCH_GAME: ItchGame = { gameId: 7, title: "Zeta", coverUrl: null, downloadKeyId: 11, url: "https://a.itch.io/zeta" };

export function fakeDownload(id: string, title: string, overrides: Partial<DownloadItem["download"]> = {}): DownloadItem {
  return {
    game: fakeGame(`dl-${id}`, title),
    download: {
      id,
      gameId: `dl-${id}`,
      source: "manual",
      status: "queued",
      totalBytes: 0,
      downloadedBytes: 0,
      bytesPerSecond: 0,
      queuePosition: 0,
      queuedAt: "2026-11-04T10:00:00Z",
      finishedAt: null,
      error: null,
      origin: "direct",
      fileName: null,
      checksumVerified: false,
      installPath: null,
      executablePath: null,
      extra: null,
      ...overrides,
    },
  };
}

/** Installs the fake core. Returns its (mutable) state for assertions. */
/**
 * Statistics like the core computes them, from the fake library: all time uses the
 * games' playtime, a year 60 % of it; months and weekdays follow fixed weights.
 */
export function fakeStats(library: LibraryItem[], year: number | null): PlayStats {
  const share = year === null ? 1 : year === 2026 ? 0.6 : year === 2025 ? 0.4 : 0;
  const played = library
    .map((item) => ({ item, minutes: Math.round(item.entry.playtimeMinutes * share) }))
    .filter((p) => p.minutes > 0)
    .sort((a, b) => b.minutes - a.minutes || a.item.game.title.localeCompare(b.item.game.title));
  const total = played.reduce((sum, p) => sum + p.minutes, 0);
  const tracked = Math.round(total * (year === null ? 0.7 : 1));
  const MONTHS = [3, 5, 4, 6, 8, 7, 5, 9, 6, 4, 7, 10];
  const WEEKDAYS = [8, 6, 7, 9, 12, 20, 18];
  const sumOf = (xs: number[]) => xs.reduce((a, b) => a + b, 0);
  const firstMonth = year === null ? { y: 2025, m: 12 } : { y: year, m: 1 };
  const months = MONTHS.map((w, i) => {
    const index = firstMonth.m - 1 + i;
    const month = `${String(firstMonth.y + Math.floor(index / 12))}-${String((index % 12) + 1).padStart(2, "0")}`;
    return { month, minutes: Math.round((tracked * w) / sumOf(MONTHS)) };
  });
  const genres = new Map<string, number>();
  const stores = new Map<StoreSource, number>();
  for (const { item, minutes } of played) {
    for (const genre of item.game.genres) genres.set(genre, (genres.get(genre) ?? 0) + minutes);
    stores.set(item.entry.source, (stores.get(item.entry.source) ?? 0) + minutes);
  }
  const sessions = played.length * 7;
  const first = played[0];
  return {
    year,
    years: total > 0 ? [2026, 2025] : [],
    totalMinutes: total,
    sessionCount: sessions,
    gamesPlayed: played.length,
    daysPlayed: Math.min(365, Math.round(sessions * 0.6)),
    longestStreakDays: played.length > 0 ? 6 : 0,
    longestSession: first ? { gameId: first.item.game.id, title: first.item.game.title, minutes: 254, startedAt: "2026-11-02T18:40:00Z" } : null,
    achievementsUnlocked: played.reduce((sum, p) => sum + p.item.entry.achievements.unlocked, 0),
    months,
    weekdays: WEEKDAYS.map((w) => Math.round((tracked * w) / sumOf(WEEKDAYS))),
    topGames: played.slice(0, 10).map(({ item, minutes }, i) => ({ game: item.game, minutes, sessionCount: 24 - i * 2 })),
    genres: [...genres].map(([genre, minutes]) => ({ genre: genre as PlayStats["genres"][number]["genre"], minutes })).sort((a, b) => b.minutes - a.minutes || a.genre.localeCompare(b.genre)),
    stores: [...stores].map(([store, minutes]) => ({ store, minutes })).sort((a, b) => b.minutes - a.minutes),
  };
}

export function installFakeCore(library: LibraryItem[] = [fakeItem("tidewater", "Tidewater")]): FakeCore {
  const core: FakeCore = {
    library: structuredClone(library),
    settings: structuredClone(DEFAULT_SETTINGS),
    calls: [],
    catalog: { cove: true, igdb: false, rawg: false, offline: false },
    candidates: {},
    recommender: true,
    accounts: { steam: fakeAccount("steam"), gog: fakeAccount("gog"), epic: fakeAccount("epic"), amazon: fakeAccount("amazon") },
    local: structuredClone(FAKE_LOCAL),
    coveAchievements: structuredClone(FAKE_COVE_ACHIEVEMENTS),
    downloads: [],
    itchKey: null,
    plugins: [],
    pluginStorage: {},
    pluginAchievements: {},
    update: null,
    updateInstalled: false,
    pickedFolder: null,
    freeBytes: {},
    moves: [],
    gogUpdate: { available: false, installedVersion: "1.0", latestVersion: "1.0" },
    launcherOnly: {},
    dlcs: {},
    installTargets: {},
    gameDlcs: {},
    collections: [],
    proton: fakeProton(),
    protonInstalls: [],
    protonRatings: {},
    protonDown: false,
    prefixes: {},
    prefixActions: [],
    timesToBeat: {},
    gameTools: { mangohud: true, gamemode: true, gamescope: false },
    backups: {},
    systems: FAKE_SYSTEMS,
    romFiles: {},
    gamepads: [],
    tvHint: false,
    fullscreen: false,
    wishlist: [],
    prices: {},
    openedUrls: [],
    itchExtras: [],
  };
  const plugin = (id: string) => core.plugins.find((p) => p.id === id) ?? reject("invalidInput", `Plugin nicht gefunden: ${id}`);
  const activate = (p: PluginInfo, enabled: boolean) => {
    p.status = enabled ? "active" : "disabled";
    p.enabled = enabled;
    p.entryUrl = enabled ? `cove-plugin://localhost/${p.id}/index.js` : null;
  };
  const requirePermission = (id: string, permission: PluginInfo["permissions"][number]) => {
    const p = plugin(id);
    if (p.status !== "active" || !p.granted.includes(permission)) reject("plugin", `Das Plugin „${p.name}“ hat keine Berechtigung für „${permission}“.`);
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

  const collection = (id: string) => core.collections.find((c) => c.id === id) ?? reject("invalidInput", "Diese Sammlung gibt es nicht mehr.");
  /** Mirrors `db::collections::checked_name` in the Rust core. */
  const collectionName = (raw: string, except?: string) => {
    const name = raw.trim().split(/\s+/).join(" ");
    if (!name) reject("invalidInput", "Bitte gib der Sammlung einen Namen.");
    if (Array.from(name).length > 50) reject("invalidInput", "Der Name ist zu lang (höchstens 50 Zeichen).");
    if (core.collections.some((c) => c.id !== except && c.name.toLowerCase() === name.toLowerCase())) {
      reject("invalidInput", `Eine Sammlung „${name}“ gibt es schon.`);
    }
    return name;
  };

  /** Mirrors `ProtonManager::overview`: which builds games start with. */
  const protonOverview = (): ProtonOverview => {
    const used = new Map<string, number>();
    for (const item of core.library) {
      const options = item.entry.launchOptions;
      if (options.compatibilityTool !== "proton") continue;
      const wanted = options.protonVersion ?? core.settings.library.defaultProton;
      const id = wanted === "latest" ? (core.proton.latest ?? wanted) : wanted;
      used.set(id, (used.get(id) ?? 0) + 1);
    }
    return {
      ...core.proton,
      umu: { ...core.proton.umu, enabled: core.settings.library.useUmu },
      versions: core.proton.versions.map((v) => {
        const games = used.get(v.id) ?? 0;
        return { ...v, games, removable: v.kind === "ge" && v.installed && games === 0 };
      }),
    };
  };
  /** Installs a GE-Proton build at once (the core downloads it in the background). */
  const installProton = (id: string) => {
    const target = id === "latest" ? core.proton.latest : id;
    const version = core.proton.versions.find((v) => v.id === target && v.kind === "ge");
    if (!version) reject("invalidInput", "Das ist keine GE-Proton-Version.");
    if (!version.installed) core.protonInstalls.push(version.id);
    version.installed = true;
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
      if (patch.launchOptions) {
        const size = (v: number | null) => v === null || (v >= 160 && v <= 15_360);
        if (!size(patch.launchOptions.tweaks.gamescopeWidth) || !size(patch.launchOptions.tweaks.gamescopeHeight)) {
          reject("invalidInput", "Die Auflösung für gamescope muss zwischen 160 und 15360 Pixeln liegen.");
        }
        item.entry.launchOptions = patch.launchOptions;
        // A GE-Proton build the user just picked is installed right away.
        const wanted = patch.launchOptions.protonVersion ?? core.settings.library.defaultProton;
        if (patch.launchOptions.compatibilityTool === "proton" && !wanted.startsWith("steam:")) installProton(wanted);
      }
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
    collections_list: () =>
      core.collections
        .map((c) => ({ ...c, gameIds: c.gameIds.filter((id) => find(id) !== null) }))
        .sort((a, b) => a.name.toLowerCase().localeCompare(b.name.toLowerCase())),
    collections_create: ({ name }) => {
      const next = String(Math.max(0, ...core.collections.map((c) => Number(c.id))) + 1);
      const created: Collection = { id: next, name: collectionName(name), gameIds: [] };
      core.collections.push(created);
      return created;
    },
    collections_rename: ({ collectionId, name }) => {
      const target = collection(collectionId);
      target.name = collectionName(name, collectionId);
      return target;
    },
    collections_delete: ({ collectionId }) => {
      collection(collectionId);
      core.collections = core.collections.filter((c) => c.id !== collectionId);
      return null;
    },
    collections_set_game: ({ collectionId, gameId, member }) => {
      const target = collection(collectionId);
      if (member) require(gameId);
      target.gameIds = target.gameIds.filter((id) => id !== gameId);
      if (member) target.gameIds.push(gameId);
      return null;
    },
    library_achievements: () => [],
    stats_overview: ({ year }) => fakeStats(core.library, year),
    gamepad_list: () => core.gamepads,
    system_tv_hint: () => core.tvHint,
    window_set_fullscreen: ({ fullscreen }) => {
      core.fullscreen = fullscreen;
      return null;
    },
    wishlist_list: () => core.wishlist.filter((w) => !core.library.some((i) => i.game.id === w.game.id || i.entry.catalogId === w.game.id)),
    wishlist_add: ({ gameId }) => {
      const game = FAKE_CATALOG.find((g) => g.id === gameId) ?? reject("catalogProvider", "Spiel nicht gefunden.");
      const existing = core.wishlist.find((w) => w.game.id === gameId);
      if (existing) return existing;
      const item: WishlistItem = { game, addedAt: new Date().toISOString(), targetPrice: null, notifyOnSale: true, price: core.prices[gameId] ?? null };
      core.wishlist.unshift(item);
      return item;
    },
    wishlist_remove: ({ gameId }) => {
      core.wishlist = core.wishlist.filter((w) => w.game.id !== gameId);
      return null;
    },
    wishlist_update: ({ gameId, targetPrice, notifyOnSale }) => {
      const item = core.wishlist.find((w) => w.game.id === gameId) ?? reject("invalidInput", "Dieses Spiel ist nicht auf deiner Wunschliste.");
      if (targetPrice !== null && (targetPrice < 0 || targetPrice > 100_000)) reject("invalidInput", "Der Wunschpreis muss zwischen 0 und 100000 liegen.");
      Object.assign(item, { targetPrice, notifyOnSale });
      return item;
    },
    wishlist_refresh: () => {
      for (const item of core.wishlist) item.price = core.prices[item.game.id] ?? null;
      return core.wishlist;
    },
    game_price: ({ gameId }) => core.prices[gameId] ?? null,
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
    game_launch: ({ gameId }) => {
      const options = require(gameId).entry.launchOptions;
      // A missing GE-Proton build is installed before the game starts.
      if (options.compatibilityTool === "proton") {
        const wanted = options.protonVersion ?? core.settings.library.defaultProton;
        if (wanted.startsWith("steam:") && !core.proton.versions.some((v) => v.id === wanted)) {
          reject("compatToolNotFound", `${wanted.slice(6)} wurde nicht gefunden. Ist es installiert?`);
        }
        if (!wanted.startsWith("steam:")) installProton(wanted);
      }
      return { gameId, method: "process", tracked: true };
    },
    proton_list: () => protonOverview(),
    proton_check: () => {
      core.proton.checkedAt = new Date().toISOString();
      return protonOverview();
    },
    proton_install: ({ versionId }) => {
      if (versionId === "umu") {
        core.proton.umu = { ...core.proton.umu, source: "cove", version: core.proton.umu.latest };
        return null;
      }
      if (versionId.startsWith("steam:")) reject("invalidInput", "Das ist keine GE-Proton-Version.");
      installProton(versionId);
      return null;
    },
    proton_remove: ({ versionId }) => {
      const version = protonOverview().versions.find((v) => v.id === versionId && v.installed);
      if (!version) reject("invalidInput", `${versionId} ist nicht installiert.`);
      if (version.kind === "steam") reject("invalidInput", "Proton-Versionen von Steam werden in Steam verwaltet.");
      if (version.games > 0) {
        reject("invalidInput", version.games === 1 ? `${versionId} wird noch von einem Spiel verwendet.` : `${versionId} wird noch von ${String(version.games)} Spielen verwendet.`);
      }
      core.proton.versions = core.proton.versions.flatMap((v) => (v.id !== versionId ? [v] : v.downloadBytes === null ? [] : [{ ...v, installed: false }]));
      return null;
    },
    system_game_tools: () => core.gameTools,
    game_backups: ({ gameId }) => {
      require(gameId);
      return core.backups[gameId] ?? { known: false, files: 0, bytes: 0, backups: [] };
    },
    game_backup_now: ({ gameId }) => {
      require(gameId);
      const state = core.backups[gameId];
      if (!state?.known) reject("invalidInput", "Für dieses Spiel kennt Cove keine Spielstand-Orte.");
      const created = { id: `2026-11-12_10-00-0${String(state.backups.length)}.000`, createdAt: new Date().toISOString(), files: state.files, bytes: state.bytes };
      state.backups = [created, ...state.backups];
      return created;
    },
    game_backup_restore: ({ gameId, backupId }) => {
      const backup = core.backups[gameId]?.backups.find((b) => b.id === backupId);
      if (!backup) reject("invalidInput", "Dieses Backup gibt es nicht.");
      return backup.files;
    },
    emulators_systems: () => core.systems,
    emulators_scan: () => {
      let found = 0;
      let added = 0;
      for (const folder of core.settings.emulators.romFolders) {
        const system = core.systems.find((s) => s.id === folder.system);
        for (const file of core.romFiles[folder.path] ?? []) {
          found += 1;
          const path = `${folder.path}/${file}`;
          if (core.library.some((i) => i.entry.executablePath === path)) continue;
          const title = file.replace(/\.[^.]+$/, "").replace(/\s*[([].*?[)\]]/g, "").trim();
          const item = fakeItem(`rom-${String(core.library.length)}`, title, { executablePath: path, playtimeMinutes: 0, lastPlayedAt: null });
          core.library.push(item);
          const name = system?.name ?? folder.system;
          let target = core.collections.find((c) => c.name === name);
          if (!target) {
            target = { id: String(core.collections.length + 1), name, gameIds: [] };
            core.collections.push(target);
          }
          target.gameIds.push(item.game.id);
          added += 1;
        }
      }
      return { found, added };
    },
    proton_cancel: ({ versionId }) => {
      if (!core.proton.installing.some((p) => p.id === versionId)) reject("invalidInput", `${versionId} wird gerade nicht installiert.`);
      core.proton.installing = core.proton.installing.filter((p) => p.id !== versionId);
      return null;
    },
    game_prefix_info: ({ gameId }) => {
      require(gameId);
      return core.prefixes[gameId] ?? { exists: false, sizeBytes: null, winetricks: true };
    },
    game_prefix_action: ({ gameId, action }) => {
      require(gameId);
      const prefix = core.prefixes[gameId];
      if (!prefix?.exists) reject("invalidInput", "Für dieses Spiel gibt es noch keinen Prefix. Er entsteht beim ersten Start mit Proton.");
      if (action === "winetricks" && !prefix.winetricks) reject("launch", "winetricks ist nicht installiert.");
      if (action === "reset") core.prefixes = Object.fromEntries(Object.entries(core.prefixes).filter(([id]) => id !== gameId));
      core.prefixActions.push({ gameId, action });
      return null;
    },
    game_stop: ({ gameId }) => {
      reject("notRunning", `${gameId} läuft nicht.`);
    },
    game_running_list: () => [],
    game_dlcs: ({ gameId }) => {
      const item = require(gameId);
      if (item.entry.source === "manual") reject("invalidInput", "Die DLC-Liste gibt es nur für Steam-, GOG- und Epic-Spiele.");
      return core.gameDlcs[gameId] ?? { dlcs: [], total: 0 };
    },
    game_proton_rating: ({ gameId }) => {
      require(gameId);
      if (core.protonDown) reject("storeUnavailable", "ProtonDB ist gerade nicht erreichbar. Versuch es später noch einmal.");
      return core.protonRatings[gameId] ?? null;
    },
    game_manage_dlcs: ({ gameId }) => {
      const item = require(gameId);
      if (item.entry.source !== "steam") reject("invalidInput", "Die DLC-Liste gibt es nur für Steam-Spiele.");
      core.openedUrls.push(`steam://nav/games/details/${item.entry.storeGameId ?? ""}`);
      return null;
    },
    game_uninstall: ({ gameId }) => {
      const item = core.library.find((i) => i.game.id === gameId) ?? reject("notInLibrary", `${gameId} ist nicht in der Bibliothek.`);
      if (item.entry.source === "steam") return "steam";
      if (!item.entry.installed) reject("invalidInput", "Dieses Spiel ist nicht installiert.");
      Object.assign(item.entry, { installed: false, installPath: null, executablePath: null, installSizeBytes: null, launchVia: null });
      return "removed";
    },
    game_move: ({ gameId, folder }) => {
      const item = core.library.find((i) => i.game.id === gameId) ?? reject("notInLibrary", `${gameId} ist nicht in der Bibliothek.`);
      if (!folder.startsWith("/")) reject("invalidInput", "Bitte wähle einen vollständigen Ordnerpfad.");
      const target = `${folder}/${item.game.title}`;
      const old = item.entry.installPath ?? "";
      item.entry.executablePath = item.entry.executablePath?.replace(old, target) ?? null;
      item.entry.installPath = target;
      core.moves.push({ gameId, folder });
      return null;
    },
    settings_get: () => core.settings,
    settings_update: ({ settings }) => {
      const proton = settings.library.defaultProton;
      if (proton !== core.settings.library.defaultProton && !proton.startsWith("steam:")) installProton(proton);
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
    plugins_list: () => core.plugins,
    plugins_set_enabled: ({ id, enabled }) => {
      const p = plugin(id);
      if (p.status === "needsApproval" && enabled) reject("plugin", "Bitte bestätige zuerst die Berechtigungen des Plugins.");
      activate(p, enabled);
      return p;
    },
    plugins_approve: ({ id }) => {
      const p = plugin(id);
      p.granted = [...p.permissions];
      activate(p, true);
      return p;
    },
    plugins_inspect: ({ path }) => {
      if (!path.includes("plugin")) reject("invalidInput", "Bitte gib den Ordner des Plugins an (mit plugin.json).");
      return { id: "csv-import", name: "CSV-Import", version: "1.0.0", author: "Cove", description: "", permissions: ["library.add", "ui.addSidebarItem", "ui.notify"], alreadyInstalled: false };
    },
    plugins_install: () => {
      const p = fakePlugin("csv-import", { name: "CSV-Import", permissions: ["library.add", "ui.addSidebarItem", "ui.notify"], removable: true });
      p.granted = [...p.permissions];
      activate(p, true);
      core.plugins.push(p);
      return p;
    },
    plugins_uninstall: ({ id }) => {
      core.plugins = core.plugins.filter((p) => p.id !== id);
      return null;
    },
    plugins_open_folder: () => null,
    plugin_storage_get: ({ pluginId, key }) => {
      requirePermission(pluginId, "storage");
      return core.pluginStorage[pluginId]?.[key] ?? null;
    },
    plugin_storage_set: ({ pluginId, key, value }) => {
      requirePermission(pluginId, "storage");
      core.pluginStorage[pluginId] = { ...core.pluginStorage[pluginId], [key]: value };
      return null;
    },
    plugin_storage_remove: ({ pluginId, key }) => {
      requirePermission(pluginId, "storage");
      core.pluginStorage[pluginId] = Object.fromEntries(Object.entries(core.pluginStorage[pluginId] ?? {}).filter(([k]) => k !== key));
      return null;
    },
    plugin_storage_keys: ({ pluginId }) => {
      requirePermission(pluginId, "storage");
      return Object.keys(core.pluginStorage[pluginId] ?? {}).sort();
    },
    plugin_define_achievements: ({ pluginId, achievements }) => {
      requirePermission(pluginId, "achievements.define");
      core.pluginAchievements[pluginId] = achievements;
      return achievements.length;
    },
    plugin_library_add: ({ pluginId, game }) => {
      requirePermission(pluginId, "library.add");
      if (core.library.some((i) => i.game.title.toLowerCase() === game.title.toLowerCase())) {
        reject("alreadyInLibrary", `„${game.title}“ ist bereits in deiner Bibliothek.`);
      }
      const item = fakeItem(`manual-${game.title.toLowerCase().replace(/\W+/g, "-")}`, game.title, {
        installed: game.executablePath !== null,
        executablePath: game.executablePath,
        playtimeMinutes: 0,
        lastPlayedAt: null,
      });
      core.library.push(item);
      return item;
    },
    downloads_list: () => core.downloads,
    downloads_enqueue: ({ gameId, target }) => {
      const item = require(gameId);
      const store = item.entry.source;
      if (store === "manual") reject("invalidInput", "Dieses Spiel stammt aus keinem Store.");
      if (store === "gog" || store === "amazon" || (store === "epic" && target)) {
        if (!target) reject("invalidInput", "Bitte wähle einen Zielordner.");
        core.installTargets[gameId] = target;
        const download = fakeDownload(`g-${gameId}`, item.game.title, { origin: store, source: store, queuePosition: core.downloads.length });
        download.download.gameId = gameId;
        download.game = item.game;
        core.downloads.push(download);
        return null;
      }
      core.downloads.push(fakeDownload(`h-${gameId}`, item.game.title, { status: "handedOff", origin: store }));
      return null;
    },
    downloads_set_status: ({ id, status }) => {
      const item = core.downloads.find((d) => d.download.id === id) ?? reject("invalidInput", "Download nicht gefunden");
      item.download.status = status;
      if (status === "cancelled") item.download.queuePosition = null;
      return null;
    },
    downloads_preview_store: ({ gameId }) => {
      const item = core.library.find((i) => i.game.id === gameId) ?? reject("notInLibrary", `${gameId} ist nicht in der Bibliothek.`);
      const store = item.entry.source;
      if (store !== "gog" && store !== "epic" && store !== "amazon") reject("invalidInput", "Cove kann nur GOG-, Epic- und Amazon-Spiele selbst installieren.");
      if (!core.accounts[store].connected) reject("invalidInput", `Du bist nicht bei ${SOURCE_NAMES[store]} angemeldet.`);
      return {
        title: item.game.title,
        downloadBytes: 9_500_000_000,
        installBytes: 12_000_000_000,
        launcherOnly: core.launcherOnly[gameId] ?? null,
        dlcs: core.dlcs[gameId] ?? [],
      };
    },
    game_update_check: ({ gameId }) => {
      const item = core.library.find((i) => i.game.id === gameId) ?? reject("notInLibrary", `${gameId} ist nicht in der Bibliothek.`);
      if (!item.entry.installedByCove) reject("invalidInput", "Cove hat dieses Spiel nicht installiert.");
      return core.gogUpdate;
    },
    game_set_dlcs: ({ gameId, excludedDlcs }) => {
      const item = require(gameId);
      if (!item.entry.installedByCove) reject("invalidInput", "Cove hat dieses Spiel nicht installiert.");
      core.dlcs[gameId] = (core.dlcs[gameId] ?? []).map((d) => ({ ...d, selected: !excludedDlcs.includes(d.id) }));
      core.downloads.push(fakeDownload(`up-${gameId}`, item.game.title, { origin: item.entry.source === "epic" ? "epic" : item.entry.source === "amazon" ? "amazon" : "gog", queuePosition: core.downloads.length }));
      return null;
    },
    game_update: ({ gameId }) => {
      const item = core.library.find((i) => i.game.id === gameId) ?? reject("notInLibrary", `${gameId} ist nicht in der Bibliothek.`);
      core.downloads.push(fakeDownload(`up-${gameId}`, item.game.title, { origin: "gog", queuePosition: core.downloads.length }));
      return null;
    },
    downloads_preview_url: ({ input }) => {
      if (!/^https?:\/\//.test(input.url)) reject("invalidInput", "Das ist kein gültiger Link.");
      const name = input.title ?? input.url.split("/").pop() ?? "Download";
      return { title: name, downloadBytes: 2_000_000_000, installBytes: null, launcherOnly: null, dlcs: [] };
    },
    downloads_add_url: ({ input }) => {
      if (!/^https?:\/\//.test(input.url)) reject("invalidInput", "Das ist kein gültiger Link.");
      const name = input.title ?? input.url.split("/").pop() ?? "Download";
      const item = fakeDownload(`u${String(core.downloads.length)}`, name, { queuePosition: core.downloads.length });
      core.downloads.push(item);
      return item;
    },
    downloads_add_itch: ({ title, extras }) => {
      if (!core.itchKey) reject("itch", "Verbinde zuerst dein itch.io-Konto.");
      core.itchExtras.push(extras);
      const item = fakeDownload(`i${String(core.downloads.length)}`, title, { origin: "itch", queuePosition: core.downloads.length });
      core.downloads.push(item);
      return item;
    },
    itch_status: () => ({ connected: core.itchKey !== null, userName: core.itchKey ? "Kiefer" : null }),
    itch_set_api_key: ({ apiKey }) => {
      if (apiKey !== "goodkey") reject("itch", "Der API-Schlüssel ist ungültig oder abgelaufen.");
      core.itchKey = apiKey;
      return { connected: true, userName: "Kiefer" };
    },
    itch_clear_api_key: () => {
      core.itchKey = null;
      return { connected: false, userName: null };
    },
    itch_owned_games: () => {
      if (!core.itchKey) reject("itch", "Verbinde zuerst dein itch.io-Konto.");
      return [FAKE_ITCH_GAME];
    },
    itch_lookup: ({ url }) => {
      if (!url.includes("itch.io")) reject("invalidInput", "Bitte gib die Adresse einer itch.io-Spielseite ein.");
      return { gameId: 99, title: "Free Game", coverUrl: null, downloadKeyId: null, url };
    },
    itch_uploads: () => [
      { uploadId: 100, fileName: "zeta-linux.zip", sizeBytes: 1_000_000, platforms: ["linux"], extra: false },
      { uploadId: 101, fileName: "zeta-soundtrack.zip", sizeBytes: 300_000_000, platforms: [], extra: true },
    ],
    downloads_reorder: () => null,
    downloads_clear_history: () => null,
    system_pick_folder: () => core.pickedFolder,
    system_folder_info: ({ path }) => {
      if (!path.startsWith("/") && !/^[A-Z]:\\/i.test(path)) reject("invalidInput", "Bitte wähle einen vollständigen Ordnerpfad.");
      return {
        path,
        freeBytes: core.freeBytes[path] ?? 500_000_000_000,
        writable: !path.startsWith("/readonly"),
        needsAdmin: /^C:\\Program Files/i.test(path),
      };
    },
    system_profile: () => ({ id: "local", displayName: "Testperson", avatarUrl: null }),
    system_about: () => ({
      appVersion: "0.1.0",
      platform: "linux",
      components: [
        { name: "core", version: "0.1.0", status: "running" },
        { name: "connectors", version: "0.1.0", status: "idle" },
        { name: "recommender", version: "0.1.0", status: core.recommender ? "running" : "unavailable" },
      ],
    }),
    updates_check: () => ({ currentVersion: "0.1.0", available: core.update, canInstall: true, installHint: null }),
    updates_install: () => {
      if (!core.update) reject("update", "Cove ist bereits auf dem neuesten Stand.");
      core.updateInstalled = true;
      return null;
    },
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
    catalog_time_to_beat: ({ catalogIds }) => {
      if (catalogIds.length > 5000) reject("invalidInput", "Zu viele Spiele auf einmal.");
      return Object.fromEntries(catalogIds.flatMap((id) => (core.timesToBeat[id] ? [[id, core.timesToBeat[id]]] : [])));
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
